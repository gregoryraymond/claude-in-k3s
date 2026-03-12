use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum Platform {
    Linux,
    MacOs,
    Wsl2,
    Windows,
}

pub fn detect_platform() -> Platform {
    if cfg!(target_os = "linux") {
        if std::fs::read_to_string("/proc/version")
            .map(|v| v.to_lowercase().contains("microsoft"))
            .unwrap_or(false)
        {
            Platform::Wsl2
        } else {
            Platform::Linux
        }
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Linux
    }
}

#[allow(dead_code)]
pub fn kubeconfig_default_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/root"))
        .join(".kube/config")
}

pub fn terraform_binary(platform: &Platform) -> &'static str {
    match platform {
        Platform::Windows => "terraform.exe",
        _ => "terraform",
    }
}

pub fn helm_binary(platform: &Platform) -> &'static str {
    match platform {
        Platform::Windows => "helm.exe",
        _ => "helm",
    }
}

pub fn kubectl_binary(platform: &Platform) -> &'static str {
    match platform {
        Platform::Windows => "kubectl.exe",
        _ => "kubectl",
    }
}

pub fn docker_binary(platform: &Platform) -> &'static str {
    match platform {
        Platform::Windows => "docker.exe",
        _ => "docker",
    }
}

/// Returns the binary name for the k8s cluster provider:
/// k3d on Windows (runs k3s inside Docker), k3s elsewhere.
pub fn k8s_provider_binary(platform: &Platform) -> &'static str {
    match platform {
        Platform::Windows => "k3d.exe",
        _ => "k3s",
    }
}

/// Display name for the k8s cluster provider (for UI labels and logs).
pub fn k8s_provider_name(platform: &Platform) -> &'static str {
    match platform {
        Platform::Windows => "k3d",
        _ => "k3s",
    }
}

pub fn platform_display_name(platform: &Platform) -> &'static str {
    match platform {
        Platform::Linux => "Linux",
        Platform::MacOs => "macOS",
        Platform::Wsl2 => "WSL2",
        Platform::Windows => "Windows",
    }
}

#[allow(dead_code)]
pub fn detect_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64" // fallback
    }
}

/// Convert a Windows path to a k3d container-compatible path.
///
/// k3d runs k3s inside a Docker container. hostPath volumes in k3d
/// reference the container's filesystem, not the Windows host.
/// Docker Desktop mounts Windows drives inside containers, so
/// `C:\Users\Greg\repos` becomes `/mnt/c/Users/Greg/repos`.
///
/// On non-Windows platforms, returns the path unchanged.
pub fn to_k3d_container_path(path: &str, platform: &Platform) -> String {
    if *platform != Platform::Windows {
        return path.to_string();
    }

    let path = path.replace('\\', "/");

    // Match "C:/..." or "c:/..."
    if path.len() >= 2 && path.as_bytes()[1] == b':' {
        let drive = (path.as_bytes()[0] as char).to_ascii_lowercase();
        let rest = &path[2..]; // "/Users/Greg/..."
        return format!("/mnt/{}{}", drive, rest);
    }

    path
}

/// Convert a k3d container path back to a Windows host path for `--volume` flags.
/// The k3d `--volume` flag expects `HOST_PATH:CONTAINER_PATH@NODEFILTER`.
/// On Windows, the host path should remain as a Windows path.
pub fn k3d_volume_flag(host_path: &str, container_path: &str) -> String {
    format!("{}:{}@server:0", host_path, container_path)
}

/// Open a native terminal window running a command inside a pod via `kubectl exec -it`.
///
/// `pod_command` is the command to run inside the pod (e.g. `/bin/sh` or `claude`).
///
/// Cross-platform:
/// - Windows: tries `wt.exe` (Windows Terminal), falls back to `cmd.exe /c start`
/// - macOS: uses `osascript` to open Terminal.app
/// - Linux/WSL2: tries common terminals in order: kitty, alacritty, gnome-terminal, konsole, xterm
pub fn open_terminal_with_kubectl_exec(
    platform: &Platform,
    kubectl_bin: &str,
    namespace: &str,
    pod_name: &str,
    pod_command: &str,
) -> std::io::Result<()> {
    let kubectl_cmd = format!(
        "{} exec -it -n {} {} -- {}",
        kubectl_bin, namespace, pod_name, pod_command
    );

    match platform {
        Platform::Windows => {
            // Try Windows Terminal first, fall back to cmd
            let wt_result = std::process::Command::new("wt.exe")
                .args(["--", "cmd", "/k", &kubectl_cmd])
                .spawn();
            match wt_result {
                Ok(_) => Ok(()),
                Err(_) => {
                    std::process::Command::new("cmd.exe")
                        .args(["/c", "start", "cmd", "/k", &kubectl_cmd])
                        .spawn()?;
                    Ok(())
                }
            }
        }
        Platform::MacOs => {
            // Use osascript to open Terminal.app with the command
            let script = format!(
                "tell application \"Terminal\" to do script \"{}\"",
                kubectl_cmd.replace('"', "\\\"")
            );
            std::process::Command::new("osascript")
                .args(["-e", &script])
                .spawn()?;
            Ok(())
        }
        Platform::Linux | Platform::Wsl2 => {
            // Try common terminal emulators in preference order
            let terminals: &[(&str, &[&str])] = &[
                ("kitty", &["--", "sh", "-c", &kubectl_cmd]),
                ("alacritty", &["-e", "sh", "-c", &kubectl_cmd]),
                ("gnome-terminal", &["--", "sh", "-c", &kubectl_cmd]),
                ("konsole", &["-e", "sh", "-c", &kubectl_cmd]),
                ("xfce4-terminal", &["-e", &kubectl_cmd]),
                ("xterm", &["-e", &kubectl_cmd]),
            ];

            for (term, args) in terminals {
                if std::process::Command::new(term).args(*args).spawn().is_ok() {
                    return Ok(());
                }
            }

            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No supported terminal emulator found. Install one of: kitty, alacritty, gnome-terminal, konsole, xfce4-terminal, xterm",
            ))
        }
    }
}

/// PRJ-55: Get available disk space in bytes for the current drive.
pub fn available_disk_space() -> Result<u64, String> {
    #[cfg(target_os = "windows")]
    {
        // Use GetDiskFreeSpaceExW via std::fs metadata approach
        // Simpler: parse `wmic` output
        let output = std::process::Command::new("wmic")
            .args(["logicaldisk", "where", "DeviceID='C:'", "get", "FreeSpace", "/value"])
            .output()
            .map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some(val) = line.strip_prefix("FreeSpace=") {
                return val.trim().parse::<u64>().map_err(|e| e.to_string());
            }
        }
        Err("Could not parse disk space".into())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let output = std::process::Command::new("df")
            .args(["-B1", "--output=avail", "/"])
            .output()
            .map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines().skip(1) {
            if let Ok(bytes) = line.trim().parse::<u64>() {
                return Ok(bytes);
            }
        }
        Err("Could not parse disk space".into())
    }
}

/// Check whether WSL2 is installed and available (Windows only).
/// Runs `wsl --status` and checks exit code.
pub async fn check_wsl_status() -> anyhow::Result<bool> {
    use std::process::Stdio as StdStdio;
    use tokio::process::Command;

    let output = Command::new("wsl")
        .args(["--status"])
        .stdout(StdStdio::piped())
        .stderr(StdStdio::piped())
        .output()
        .await?;

    Ok(output.status.success())
}

/// Restart WSL (Windows only). Shuts down all WSL instances so they
/// reinitialize on next use.
pub async fn restart_wsl() -> anyhow::Result<()> {
    use std::process::Stdio as StdStdio;
    use tokio::process::Command;

    Command::new("wsl")
        .args(["--shutdown"])
        .stdout(StdStdio::null())
        .stderr(StdStdio::null())
        .output()
        .await?;
    Ok(())
}

/// Launch Docker Desktop (Windows only).
/// Uses `docker desktop start` (Docker Desktop 4.37+).
/// Falls back to launching the exe directly if the CLI subcommand isn't available.
pub async fn start_docker_desktop() -> anyhow::Result<()> {
    use std::process::Stdio as StdStdio;
    use tokio::process::Command;

    let result = Command::new("docker")
        .args(["desktop", "start"])
        .stdout(StdStdio::null())
        .stderr(StdStdio::piped())
        .output()
        .await;

    match result {
        Ok(output) if output.status.success() => return Ok(()),
        _ => {}
    }

    // Fallback: launch Docker Desktop exe directly
    // Common install locations
    for path in &[
        r"C:\Program Files\Docker\Docker\Docker Desktop.exe",
        r"C:\Program Files (x86)\Docker\Docker\Docker Desktop.exe",
    ] {
        if std::path::Path::new(path).exists() {
            Command::new(path)
                .stdout(StdStdio::null())
                .stderr(StdStdio::null())
                .spawn()?;
            return Ok(());
        }
    }

    anyhow::bail!("Docker Desktop not found")
}

/// Stop Docker Desktop (Windows only).
/// Tries `docker desktop stop` with a timeout, then force-kills if stuck.
pub async fn stop_docker_desktop() -> anyhow::Result<()> {
    use std::process::Stdio as StdStdio;
    use tokio::process::Command;

    // Try graceful stop with 15s timeout
    let graceful = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        Command::new("docker")
            .args(["desktop", "stop"])
            .stdout(StdStdio::null())
            .stderr(StdStdio::null())
            .output(),
    ).await;

    if let Ok(Ok(output)) = graceful {
        if output.status.success() {
            return Ok(());
        }
    }

    // Graceful stop failed or timed out — force kill everything
    // Kill all Docker-related processes
    for proc_name in &[
        "Docker Desktop.exe",
        "com.docker.backend.exe",
        "com.docker.build.exe",
        "docker-desktop.exe",
    ] {
        match Command::new("taskkill")
            .args(["/F", "/IM", proc_name, "/T"])
            .stdout(StdStdio::null())
            .stderr(StdStdio::null())
            .output()
            .await
        {
            Ok(output) if !output.status.success() => {
                tracing::debug!("taskkill {} returned non-zero (may not be running)", proc_name);
            }
            Err(e) => tracing::warn!("Failed to taskkill {}: {}", proc_name, e),
            _ => {}
        }
    }

    // Stop WSL docker-desktop distros and shut down WSL entirely
    if let Err(e) = Command::new("wsl")
        .args(["-t", "docker-desktop"])
        .stdout(StdStdio::null())
        .stderr(StdStdio::null())
        .output()
        .await
    {
        tracing::warn!("Failed to terminate WSL docker-desktop distro: {}", e);
    }
    if let Err(e) = Command::new("wsl")
        .args(["--shutdown"])
        .stdout(StdStdio::null())
        .stderr(StdStdio::null())
        .output()
        .await
    {
        tracing::warn!("Failed to shutdown WSL: {}", e);
    }

    // Restart the WslService in case it's stuck
    // net stop/start requires elevation but will silently fail if not admin
    if let Err(e) = Command::new("net")
        .args(["stop", "WslService"])
        .stdout(StdStdio::null())
        .stderr(StdStdio::null())
        .output()
        .await
    {
        tracing::debug!("net stop WslService failed (may need elevation): {}", e);
    }
    if let Err(e) = Command::new("net")
        .args(["start", "WslService"])
        .stdout(StdStdio::null())
        .stderr(StdStdio::null())
        .output()
        .await
    {
        tracing::debug!("net start WslService failed (may need elevation): {}", e);
    }

    // Give everything time to fully shut down
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_debug_and_clone() {
        let p = Platform::Linux;
        let cloned = p.clone();
        assert_eq!(p, cloned);
        // Verify Debug is implemented by formatting
        let debug_str = format!("{:?}", p);
        assert!(!debug_str.is_empty());
    }

    #[test]
    fn detect_platform_returns_a_variant() {
        let _platform = detect_platform();
    }

    #[test]
    fn terraform_binary_linux() {
        assert_eq!(terraform_binary(&Platform::Linux), "terraform");
    }

    #[test]
    fn terraform_binary_macos() {
        assert_eq!(terraform_binary(&Platform::MacOs), "terraform");
    }

    #[test]
    fn terraform_binary_wsl2() {
        assert_eq!(terraform_binary(&Platform::Wsl2), "terraform");
    }

    #[test]
    fn terraform_binary_windows() {
        assert_eq!(terraform_binary(&Platform::Windows), "terraform.exe");
    }

    #[test]
    fn helm_binary_linux() {
        assert_eq!(helm_binary(&Platform::Linux), "helm");
    }

    #[test]
    fn helm_binary_windows() {
        assert_eq!(helm_binary(&Platform::Windows), "helm.exe");
    }

    #[test]
    fn kubectl_binary_linux() {
        assert_eq!(kubectl_binary(&Platform::Linux), "kubectl");
    }

    #[test]
    fn kubectl_binary_windows() {
        assert_eq!(kubectl_binary(&Platform::Windows), "kubectl.exe");
    }

    #[test]
    fn docker_binary_linux() {
        assert_eq!(docker_binary(&Platform::Linux), "docker");
    }

    #[test]
    fn docker_binary_windows() {
        assert_eq!(docker_binary(&Platform::Windows), "docker.exe");
    }

    #[test]
    fn platform_display_names() {
        assert_eq!(platform_display_name(&Platform::Linux), "Linux");
        assert_eq!(platform_display_name(&Platform::MacOs), "macOS");
        assert_eq!(platform_display_name(&Platform::Wsl2), "WSL2");
        assert_eq!(platform_display_name(&Platform::Windows), "Windows");
    }

    #[test]
    fn kubeconfig_path_ends_with_kube_config() {
        let path = kubeconfig_default_path();
        assert!(path.ends_with(".kube/config"));
    }

    #[test]
    fn detect_arch_returns_known_value() {
        let arch = detect_arch();
        assert!(
            arch == "x86_64" || arch == "aarch64" || arch == "arm64",
            "unexpected arch: {}",
            arch
        );
    }

    #[test]
    fn k8s_provider_binary_linux() {
        assert_eq!(k8s_provider_binary(&Platform::Linux), "k3s");
    }

    #[test]
    fn k8s_provider_binary_windows() {
        assert_eq!(k8s_provider_binary(&Platform::Windows), "k3d.exe");
    }

    #[test]
    fn k8s_provider_name_linux() {
        assert_eq!(k8s_provider_name(&Platform::Linux), "k3s");
    }

    #[test]
    fn k8s_provider_name_windows() {
        assert_eq!(k8s_provider_name(&Platform::Windows), "k3d");
    }

    #[test]
    fn to_k3d_path_windows_backslashes() {
        let result = to_k3d_container_path(r"C:\Users\Greg\repos\day-trading", &Platform::Windows);
        assert_eq!(result, "/mnt/c/Users/Greg/repos/day-trading");
    }

    #[test]
    fn to_k3d_path_windows_forward_slashes() {
        let result = to_k3d_container_path("C:/Users/Greg/.claude", &Platform::Windows);
        assert_eq!(result, "/mnt/c/Users/Greg/.claude");
    }

    #[test]
    fn to_k3d_path_lowercase_drive() {
        let result = to_k3d_container_path(r"d:\data\projects", &Platform::Windows);
        assert_eq!(result, "/mnt/d/data/projects");
    }

    #[test]
    fn to_k3d_path_linux_unchanged() {
        let result = to_k3d_container_path("/home/user/projects", &Platform::Linux);
        assert_eq!(result, "/home/user/projects");
    }

    #[test]
    fn k3d_volume_flag_format() {
        let flag = k3d_volume_flag(r"C:\Users\Greg\repos", "/mnt/c/Users/Greg/repos");
        assert_eq!(flag, r"C:\Users\Greg\repos:/mnt/c/Users/Greg/repos@server:0");
    }
}
