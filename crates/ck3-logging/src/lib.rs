//! Rolling file + stderr logging for Claude-in-K3s.
//!
//! Call [`init`] once at startup. It returns a [`LogGuard`] that **must** be
//! held alive for the lifetime of the program — dropping it flushes and
//! closes the log file.
//!
//! ```no_run
//! let _log = ck3_logging::init("claude-in-k3s", "claude_in_k3s=info");
//! tracing::info!("ready");
//! ```

use std::path::{Path, PathBuf};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Holds the background writer thread alive.  Drop only at program exit.
pub struct LogGuard {
    _guard: WorkerGuard,
    log_dir: PathBuf,
}

impl LogGuard {
    /// Directory where log files are written.
    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }
}

/// Initialise tracing with both stderr and daily-rolling-file output.
///
/// * `app_name`  — used to build the log directory and file prefix
///   (e.g. `"claude-in-k3s"` → `claude-in-k3s.log.2025-03-09`).
/// * `default_filter` — tracing `EnvFilter` directive applied unless
///   `RUST_LOG` overrides it (e.g. `"claude_in_k3s=info"`).
///
/// Log directory:
/// - Windows: `%APPDATA%/{app_name}/logs/`
/// - Others:  `~/.local/share/{app_name}/logs/`
///
/// Old log files (> 7 days) are deleted on every call.
pub fn init(app_name: &str, default_filter: &str) -> LogGuard {
    let log_dir = log_dir(app_name);
    std::fs::create_dir_all(&log_dir).ok();
    cleanup_old_logs(&log_dir, 7);

    let file_appender =
        tracing_appender::rolling::daily(&log_dir, format!("{app_name}.log"));
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(
            EnvFilter::from_default_env()
                .add_directive(default_filter.parse().expect("valid filter directive")),
        )
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(fmt::layer().with_writer(non_blocking).with_ansi(false))
        .init();

    LogGuard {
        _guard: guard,
        log_dir,
    }
}

/// Return the platform-appropriate directory for log files.
pub fn log_dir(app_name: &str) -> PathBuf {
    if cfg!(windows) {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(base).join(app_name).join("logs")
    } else {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(app_name)
            .join("logs")
    }
}

/// Delete log files older than `max_age_days` days.
pub fn cleanup_old_logs(log_dir: &Path, max_age_days: u64) {
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return;
    };
    let cutoff =
        std::time::SystemTime::now() - std::time::Duration::from_secs(max_age_days * 24 * 3600);
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if modified < cutoff {
            if let Err(e) = std::fs::remove_file(entry.path()) {
                eprintln!("Failed to remove old log file {:?}: {e}", entry.path());
            }
        }
    }

    // LOG-3: Also enforce total size limit (100 MB)
    cleanup_by_size(log_dir, 100 * 1024 * 1024);
}

/// LOG-3: Delete oldest log files until total size is under `max_bytes`.
pub fn cleanup_by_size(log_dir: &Path, max_bytes: u64) {
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return;
    };

    // Collect files with metadata
    let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = entries
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            if meta.is_file() {
                let modified = meta.modified().ok()?;
                Some((e.path(), meta.len(), modified))
            } else {
                None
            }
        })
        .collect();

    let total_size: u64 = files.iter().map(|(_, size, _)| size).sum();
    if total_size <= max_bytes {
        return;
    }

    // Sort oldest first
    files.sort_by_key(|(_, _, modified)| *modified);

    let mut remaining = total_size;
    for (path, size, _) in &files {
        if remaining <= max_bytes {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            remaining -= size;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_dir_contains_app_name() {
        let dir = log_dir("my-app");
        assert!(
            dir.to_string_lossy().contains("my-app"),
            "log dir should contain app name: {dir:?}"
        );
        assert!(
            dir.to_string_lossy().contains("logs"),
            "log dir should end with logs: {dir:?}"
        );
    }

    #[test]
    fn cleanup_ignores_missing_dir() {
        // Should not panic when directory doesn't exist.
        cleanup_old_logs(Path::new("/nonexistent/path/that/does/not/exist"), 7);
    }

    #[test]
    fn cleanup_removes_old_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let old_file = tmp.path().join("old.log");
        std::fs::write(&old_file, "old").unwrap();

        // Backdate the file by setting modified time far in the past.
        // We can't easily set mtime portably, so instead use max_age_days=0
        // which means "delete everything".
        cleanup_old_logs(tmp.path(), 0);
        assert!(!old_file.exists(), "old file should be deleted");
    }

    #[test]
    fn cleanup_keeps_recent_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let recent = tmp.path().join("recent.log");
        std::fs::write(&recent, "fresh").unwrap();

        cleanup_old_logs(tmp.path(), 7);
        assert!(recent.exists(), "recent file should be kept");
    }

    // =========================================================================
    // LOG-3: Size-based cleanup
    // =========================================================================

    #[test]
    fn cleanup_by_size_under_limit_keeps_all() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.log"), "small").unwrap();
        std::fs::write(tmp.path().join("b.log"), "small").unwrap();

        cleanup_by_size(tmp.path(), 1024 * 1024); // 1 MB limit
        assert!(tmp.path().join("a.log").exists());
        assert!(tmp.path().join("b.log").exists());
    }

    #[test]
    fn cleanup_by_size_removes_oldest_when_over() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Create two files, each 100 bytes, with a 10-byte limit
        let data = "x".repeat(100);
        std::fs::write(tmp.path().join("old.log"), &data).unwrap();
        // Small delay so modified times differ
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(tmp.path().join("new.log"), &data).unwrap();

        cleanup_by_size(tmp.path(), 150); // 150 bytes limit, total is 200
        // Should have deleted the older file
        let remaining: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .flatten()
            .collect();
        assert!(remaining.len() <= 1, "should have deleted at least one file, remaining: {}", remaining.len());
    }

    #[test]
    fn cleanup_by_size_handles_empty_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        cleanup_by_size(tmp.path(), 100); // should not panic
    }

    #[test]
    fn cleanup_by_size_handles_nonexistent_dir() {
        cleanup_by_size(Path::new("/nonexistent/dir"), 100); // should not panic
    }
}
