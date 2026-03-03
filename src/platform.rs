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
