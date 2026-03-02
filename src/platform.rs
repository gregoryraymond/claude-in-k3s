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

pub fn platform_display_name(platform: &Platform) -> &'static str {
    match platform {
        Platform::Linux => "Linux",
        Platform::MacOs => "macOS",
        Platform::Wsl2 => "WSL2",
        Platform::Windows => "Windows",
    }
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
}
