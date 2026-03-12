//! Cross-platform test utilities for mock CLI scripts.
//!
//! On Unix: creates bash scripts with `chmod +x`.
//! On Windows: creates `.bat` files (which are natively executable).
use std::fs;
use std::path::{Path, PathBuf};

/// Creates a mock executable that outputs the given stdout text and exits with the given code.
/// Returns the path to the mock executable.
pub fn create_mock_binary(dir: &Path, name: &str, stdout: &str, exit_code: i32) -> PathBuf {
    create_mock_binary_with_stderr(dir, name, stdout, "", exit_code)
}

/// Creates a mock executable that outputs given stdout and stderr text, then exits.
/// Returns the path to the mock executable.
pub fn create_mock_binary_with_stderr(
    dir: &Path,
    name: &str,
    stdout: &str,
    stderr: &str,
    exit_code: i32,
) -> PathBuf {
    #[cfg(unix)]
    {
        let path = dir.join(name);
        let mut script = String::from("#!/bin/sh\n");
        for line in stdout.lines() {
            script.push_str(&format!("echo '{}'\n", line.replace('\'', "'\\''")));
        }
        for line in stderr.lines() {
            script.push_str(&format!("echo '{}' >&2\n", line.replace('\'', "'\\''")));
        }
        script.push_str(&format!("exit {}\n", exit_code));
        fs::write(&path, &script).unwrap();

        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
    #[cfg(windows)]
    {
        let path = dir.join(format!("{}.bat", name));
        let mut script = String::from("@echo off\r\n");
        for line in stdout.lines() {
            if line.is_empty() {
                script.push_str("echo.\r\n");
            } else {
                script.push_str(&format!("echo {}\r\n", line));
            }
        }
        for line in stderr.lines() {
            if line.is_empty() {
                script.push_str("echo. >&2\r\n");
            } else {
                script.push_str(&format!("echo {} >&2\r\n", line));
            }
        }
        script.push_str(&format!("exit /b {}\r\n", exit_code));
        fs::write(&path, &script).unwrap();
        path
    }
}

/// Creates a mock executable that records its arguments to a file, outputs stdout, and exits.
/// Returns `(executable_path, args_file_path)`.
pub fn create_mock_binary_recording(
    dir: &Path,
    name: &str,
    stdout: &str,
    exit_code: i32,
) -> (PathBuf, PathBuf) {
    let args_file = dir.join(format!("{}_args.txt", name));

    #[cfg(unix)]
    {
        let path = dir.join(name);
        let stdout_lines = stdout
            .lines()
            .map(|l| format!("echo '{}'", l.replace('\'', "'\\''")))
            .collect::<Vec<_>>()
            .join("\n");
        let script = format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\n{}\nexit {}\n",
            args_file.display(),
            stdout_lines,
            exit_code
        );
        fs::write(&path, &script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        (path, args_file)
    }
    #[cfg(windows)]
    {
        let path = dir.join(format!("{}.bat", name));
        let args_path_str = args_file.to_string_lossy().replace('/', "\\");
        let stdout_lines = stdout
            .lines()
            .map(|l| {
                if l.is_empty() {
                    "echo.".to_string()
                } else {
                    format!("echo {}", l)
                }
            })
            .collect::<Vec<_>>()
            .join("\r\n");
        let script = format!(
            "@echo off\r\necho %* >> \"{}\"\r\n{}\r\nexit /b {}\r\n",
            args_path_str, stdout_lines, exit_code
        );
        fs::write(&path, &script).unwrap();
        (path, args_file)
    }
}
