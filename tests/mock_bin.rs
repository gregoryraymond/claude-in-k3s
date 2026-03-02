use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Creates a mock executable script in a temp directory that echoes canned output.
/// Returns (TempDir, path_to_script). TempDir must be kept alive for the script to exist.
pub fn create_mock_binary(name: &str, script_body: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let script_path = dir.path().join(name);
    let content = format!("#!/bin/bash\n{}", script_body);
    fs::write(&script_path, &content).expect("failed to write mock script");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))
            .expect("failed to chmod mock script");
    }

    (dir, script_path)
}

/// Creates a mock binary that always succeeds with given stdout.
pub fn mock_success(name: &str, stdout: &str) -> (TempDir, PathBuf) {
    let body = format!("echo '{}'\nexit 0", stdout.replace('\'', "'\\''"));
    create_mock_binary(name, &body)
}

/// Creates a mock binary that always fails with given stderr.
pub fn mock_failure(name: &str, stderr: &str) -> (TempDir, PathBuf) {
    let body = format!("echo '{}' >&2\nexit 1", stderr.replace('\'', "'\\''"));
    create_mock_binary(name, &body)
}

/// Creates a mock binary that writes its args to a file, then outputs canned response.
/// Useful for verifying the runner passed correct arguments.
pub fn mock_record_args(name: &str, stdout: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let script_path = dir.path().join(name);
    let args_file = dir.path().join("recorded_args");
    let content = format!(
        "#!/bin/bash\necho \"$@\" >> {}\necho '{}'\nexit 0",
        args_file.display(),
        stdout.replace('\'', "'\\''")
    );
    fs::write(&script_path, &content).expect("failed to write mock script");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))
            .expect("failed to chmod mock script");
    }

    (dir, script_path)
}

/// Read the recorded args file from a mock_record_args temp dir.
pub fn read_recorded_args(dir: &Path) -> Vec<String> {
    let args_file = dir.join("recorded_args");
    if args_file.exists() {
        fs::read_to_string(&args_file)
            .unwrap_or_default()
            .lines()
            .map(|l| l.to_string())
            .collect()
    } else {
        vec![]
    }
}
