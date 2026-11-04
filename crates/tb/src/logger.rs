//! A small `log` backend that appends to the session's log file.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use log::{LevelFilter, Log, Metadata, Record};

struct FileLogger {
    file: Mutex<File>,
    level: LevelFilter,
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        if let Ok(mut f) = self.file.lock() {
            let _ = writeln!(
                f,
                "{}.{:03} {:5} {}: {}",
                now.as_secs(),
                now.subsec_millis(),
                record.level(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {
        if let Ok(mut f) = self.file.lock() {
            let _ = f.flush();
        }
    }
}

/// Logs to `path`. The level comes from `TB_LOG` (error, warn, info, debug, trace); the default is info.
pub fn init(path: &Path) -> std::io::Result<()> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let level = std::env::var("TB_LOG").ok().and_then(|l| l.parse().ok()).unwrap_or(LevelFilter::Info);
    let logger = FileLogger { file: Mutex::new(file), level };
    if log::set_logger(Box::leak(Box::new(logger))).is_ok() {
        log::set_max_level(level);
    }
    Ok(())
}
