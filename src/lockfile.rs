use std::fs;
use std::path::PathBuf;

/// Returns the lockfile path under the app config directory.
pub fn lockfile_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("claude-in-k3s")
        .join("app.lock")
}

/// Attempt to acquire the singleton lockfile.
/// Returns `Ok(())` if this is the only instance, `Err(msg)` if another is running.
pub fn acquire() -> Result<(), String> {
    let path = lockfile_path();

    if path.exists() {
        // Check if the PID in the lockfile is still alive
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(pid) = content.trim().parse::<u32>() {
                if is_pid_alive(pid) {
                    return Err(format!(
                        "Another instance of Claude in K3s is already running (PID {}). Only one instance is allowed.",
                        pid
                    ));
                }
                // Stale lockfile — previous process died without cleanup
                tracing::info!("Removing stale lockfile (PID {} no longer running)", pid);
            }
        }
    }

    // Write our PID
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let pid = std::process::id();
    fs::write(&path, pid.to_string())
        .map_err(|e| format!("Failed to create lockfile: {}", e))?;

    Ok(())
}

/// Release the lockfile on shutdown.
pub fn release() {
    let path = lockfile_path();
    if path.exists() {
        fs::remove_file(&path).ok();
    }
}

/// Check if a process with the given PID is still running.
fn is_pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use std::process::Command;
        // tasklist /FI "PID eq <pid>" returns process info if alive
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output()
            .map(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                // If the PID is found, tasklist shows the process name
                !stdout.contains("No tasks") && stdout.contains(&pid.to_string())
            })
            .unwrap_or(false)
    }

    #[cfg(not(windows))]
    {
        // On Unix, sending signal 0 checks if process exists
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn lockfile_path_is_under_config_dir() {
        let path = lockfile_path();
        assert!(path.to_string_lossy().contains("claude-in-k3s"));
        assert!(path.to_string_lossy().ends_with("app.lock"));
    }

    #[test]
    fn is_pid_alive_current_process() {
        let pid = std::process::id();
        assert!(is_pid_alive(pid), "current process should be alive");
    }

    #[test]
    fn is_pid_alive_nonexistent() {
        // PID 99999999 is extremely unlikely to exist
        assert!(!is_pid_alive(99999999));
    }

    #[test]
    fn acquire_and_release() {
        // This tests the core logic but uses the real path.
        // In a real test suite we'd mock the path, but for now
        // just verify acquire succeeds and release cleans up.
        let result = acquire();
        assert!(result.is_ok(), "acquire should succeed: {:?}", result);

        let path = lockfile_path();
        assert!(path.exists(), "lockfile should exist after acquire");

        release();
        assert!(!path.exists(), "lockfile should be removed after release");
    }

    #[test]
    fn stale_lockfile_is_cleaned_up() {
        let path = lockfile_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        // Write a fake PID that's definitely not running
        fs::write(&path, "99999999").ok();

        // acquire should succeed because PID 99999999 isn't alive
        let result = acquire();
        assert!(result.is_ok(), "should clean stale lockfile: {:?}", result);

        release();
    }
}
