use crate::platform::{self, Platform};
use std::process::Command;
use tokio::process::Command as AsyncCommand;

#[derive(Debug, Clone, PartialEq)]
pub enum ToolStatus {
    Found { version: String },
    Missing,
}

impl ToolStatus {
    pub fn is_found(&self) -> bool {
        matches!(self, ToolStatus::Found { .. })
    }
}

#[derive(Debug, Clone)]
pub struct DepsStatus {
    pub k3s: ToolStatus,
    pub terraform: ToolStatus,
    pub helm: ToolStatus,
    pub docker: ToolStatus,
    pub claude: ToolStatus,
}

impl DepsStatus {
    pub fn all_met(&self) -> bool {
        self.k3s.is_found()
            && self.terraform.is_found()
            && self.helm.is_found()
            && self.docker.is_found()
    }
}

/// Check if a single tool is available on PATH.
pub fn check_tool(binary: &str) -> ToolStatus {
    let which_result = Command::new("which")
        .arg(binary)
        .output();

    match which_result {
        Ok(output) if output.status.success() => {
            // Try to get version
            let version = Command::new(binary)
                .arg("version")
                .output()
                .ok()
                .and_then(|o| {
                    let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if out.is_empty() {
                        let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                        if err.is_empty() { None } else { Some(err) }
                    } else {
                        Some(out)
                    }
                })
                .unwrap_or_else(|| "unknown".to_string());

            let version = version.lines().next().unwrap_or("unknown").to_string();
            ToolStatus::Found { version }
        }
        _ => ToolStatus::Missing,
    }
}

/// Check all 4 required tools for the given platform.
pub fn check_all(platform: &Platform) -> DepsStatus {
    DepsStatus {
        k3s: check_tool(platform::k8s_provider_binary(platform)),
        terraform: check_tool(platform::terraform_binary(platform)),
        helm: check_tool(platform::helm_binary(platform)),
        docker: check_tool(platform::docker_binary(platform)),
        claude: check_tool("claude"),
    }
}

/// URL for downloading terraform binary
pub fn terraform_download_url(arch: &str, platform: &Platform) -> String {
    let os = match platform {
        Platform::Windows => "windows",
        Platform::MacOs => "darwin",
        _ => "linux",
    };
    let arch_suffix = match arch {
        "aarch64" => "arm64",
        _ => "amd64",
    };
    format!(
        "https://releases.hashicorp.com/terraform/1.9.8/terraform_1.9.8_{}_{}.zip",
        os, arch_suffix
    )
}

/// URL for downloading helm binary
pub fn helm_download_url(arch: &str, platform: &Platform) -> String {
    let os = match platform {
        Platform::Windows => "windows",
        Platform::MacOs => "darwin",
        _ => "linux",
    };
    let arch_suffix = match arch {
        "aarch64" => "arm64",
        _ => "amd64",
    };
    let ext = match platform {
        Platform::Windows => "zip",
        _ => "tar.gz",
    };
    format!(
        "https://get.helm.sh/helm-v3.16.4-{}-{}.{}",
        os, arch_suffix, ext
    )
}

/// Install terraform by downloading the binary to ~/.local/bin/
pub async fn install_terraform() -> Result<String, String> {
    let platform = crate::platform::detect_platform();
    let arch = crate::platform::detect_arch();
    let url = terraform_download_url(arch, &platform);
    let install_dir = local_bin_dir()?;
    ensure_dir(&install_dir)?;

    let tmp_dir = std::env::temp_dir().join("claude-k3s-terraform-install");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("mkdir failed: {}", e))?;

    let zip_path = tmp_dir.join("terraform.zip");

    run_async(&format!("curl -fsSL -o {} {}", shell_path(&zip_path), url)).await?;
    run_async(&format!("unzip -o {} -d {}", shell_path(&zip_path), shell_path(&tmp_dir))).await?;

    let bin_name = crate::platform::terraform_binary(&platform);
    let src = tmp_dir.join(bin_name);
    let dst = install_dir.join(bin_name);
    std::fs::copy(&src, &dst).map_err(|e| format!("copy failed: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod failed: {}", e))?;
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);
    Ok(format!("Terraform installed to {}", dst.display()))
}

/// Install helm by downloading the binary to ~/.local/bin/
pub async fn install_helm() -> Result<String, String> {
    let platform = crate::platform::detect_platform();
    let arch = crate::platform::detect_arch();
    let url = helm_download_url(arch, &platform);
    let install_dir = local_bin_dir()?;
    ensure_dir(&install_dir)?;

    let tmp_dir = std::env::temp_dir().join("claude-k3s-helm-install");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("mkdir failed: {}", e))?;

    let is_windows = matches!(platform, Platform::Windows);
    let archive_name = if is_windows { "helm.zip" } else { "helm.tar.gz" };
    let archive_path = tmp_dir.join(archive_name);

    run_async(&format!("curl -fsSL -o {} {}", shell_path(&archive_path), url)).await?;

    if is_windows {
        run_async(&format!("unzip -o {} -d {}", shell_path(&archive_path), shell_path(&tmp_dir))).await?;
    } else {
        run_async(&format!("tar xzf {} -C {}", shell_path(&archive_path), shell_path(&tmp_dir))).await?;
    }

    let (os_name, arch_name) = match (&platform, arch) {
        (Platform::Windows, "aarch64") => ("windows", "arm64"),
        (Platform::Windows, _) => ("windows", "amd64"),
        (Platform::MacOs, "aarch64") => ("darwin", "arm64"),
        (Platform::MacOs, _) => ("darwin", "amd64"),
        (_, "aarch64") => ("linux", "arm64"),
        _ => ("linux", "amd64"),
    };
    let arch_dir = format!("{}-{}", os_name, arch_name);
    let bin_name = crate::platform::helm_binary(&platform);
    let src = tmp_dir.join(&arch_dir).join(bin_name);
    let dst = install_dir.join(bin_name);
    std::fs::copy(&src, &dst).map_err(|e| format!("copy failed: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod failed: {}", e))?;
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);
    Ok(format!("Helm installed to {}", dst.display()))
}

/// URL for downloading k3d binary (used on Windows where k3s can't run natively)
pub fn k3d_download_url(arch: &str) -> String {
    let arch_suffix = match arch {
        "aarch64" => "arm64",
        _ => "amd64",
    };
    format!(
        "https://github.com/k3d-io/k3d/releases/download/v5.7.5/k3d-windows-{}.exe",
        arch_suffix
    )
}

/// Install k3s (Linux/macOS/WSL2) or k3d (Windows).
pub async fn install_k3s() -> Result<String, String> {
    let platform = crate::platform::detect_platform();
    match platform {
        Platform::Windows => install_k3d().await,
        _ => run_async("curl -sfL https://get.k3s.io | sudo sh -").await,
    }
}

/// Install k3d by downloading the binary to ~/.local/bin/ (Windows)
async fn install_k3d() -> Result<String, String> {
    let arch = crate::platform::detect_arch();
    let url = k3d_download_url(arch);
    let install_dir = local_bin_dir()?;
    ensure_dir(&install_dir)?;

    let dst = install_dir.join("k3d.exe");
    run_async(&format!("curl -fsSL -o {} {}", shell_path(&dst), url)).await?;

    Ok(format!("k3d installed to {}", dst.display()))
}

/// Install docker using the official install script (requires sudo)
pub async fn install_docker() -> Result<String, String> {
    let platform = crate::platform::detect_platform();
    match platform {
        Platform::Windows => Err(
            "Please install Docker Desktop from \
             https://www.docker.com/products/docker-desktop/"
                .to_string(),
        ),
        _ => run_async("curl -fsSL https://get.docker.com | sudo sh").await,
    }
}

fn local_bin_dir() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;
    Ok(home.join(".local").join("bin"))
}

fn ensure_dir(path: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("Failed to create {}: {}", path.display(), e))
}

/// Convert a path to a shell-safe string with forward slashes.
/// On Windows/MINGW, backslash paths break shell commands (backslashes are
/// interpreted as escapes, and colons as remote-host prefixes by tar).
fn shell_path(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

async fn run_async(cmd: &str) -> Result<String, String> {
    let output = AsyncCommand::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .await
        .map_err(|e| format!("Failed to run '{}': {}", cmd, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        Err(format!("Command failed: {}\n{}", cmd, stderr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_status_is_found() {
        let found = ToolStatus::Found { version: "1.0".into() };
        assert!(found.is_found());
        let missing = ToolStatus::Missing;
        assert!(!missing.is_found());
    }

    #[test]
    fn deps_status_all_met_true() {
        let status = DepsStatus {
            k3s: ToolStatus::Found { version: "v1.28".into() },
            terraform: ToolStatus::Found { version: "1.5.0".into() },
            helm: ToolStatus::Found { version: "3.12".into() },
            docker: ToolStatus::Found { version: "24.0".into() },
            claude: ToolStatus::Found { version: "1.0".into() },
        };
        assert!(status.all_met());
    }

    #[test]
    fn deps_status_all_met_false_when_missing() {
        let status = DepsStatus {
            k3s: ToolStatus::Found { version: "v1.28".into() },
            terraform: ToolStatus::Missing,
            helm: ToolStatus::Found { version: "3.12".into() },
            docker: ToolStatus::Found { version: "24.0".into() },
            claude: ToolStatus::Missing,
        };
        assert!(!status.all_met());
    }

    #[test]
    fn check_tool_nonexistent_binary() {
        let status = check_tool("this_tool_does_not_exist_xyz_12345");
        assert_eq!(status, ToolStatus::Missing);
    }

    #[test]
    fn check_tool_existing_binary() {
        // `ls` exists on every Linux/macOS system
        let status = check_tool("ls");
        assert!(status.is_found());
    }

    #[test]
    fn check_all_returns_status_for_all_tools() {
        let status = check_all(&Platform::Linux);
        let _ = status.k3s;
        let _ = status.terraform;
        let _ = status.helm;
        let _ = status.docker;
    }

    #[test]
    fn terraform_download_url_linux_x86_64() {
        let url = terraform_download_url("x86_64", &Platform::Linux);
        assert!(url.contains("terraform"));
        assert!(url.contains("linux_amd64"));
    }

    #[test]
    fn terraform_download_url_linux_aarch64() {
        let url = terraform_download_url("aarch64", &Platform::Linux);
        assert!(url.contains("terraform"));
        assert!(url.contains("linux_arm64"));
    }

    #[test]
    fn terraform_download_url_windows() {
        let url = terraform_download_url("x86_64", &Platform::Windows);
        assert!(url.contains("windows_amd64"));
    }

    #[test]
    fn helm_download_url_linux_x86_64() {
        let url = helm_download_url("x86_64", &Platform::Linux);
        assert!(url.contains("helm"));
        assert!(url.contains("linux-amd64"));
        assert!(url.ends_with(".tar.gz"));
    }

    #[test]
    fn helm_download_url_linux_aarch64() {
        let url = helm_download_url("aarch64", &Platform::Linux);
        assert!(url.contains("helm"));
        assert!(url.contains("linux-arm64"));
    }

    #[test]
    fn helm_download_url_windows() {
        let url = helm_download_url("x86_64", &Platform::Windows);
        assert!(url.contains("windows-amd64"));
        assert!(url.ends_with(".zip"));
    }

    #[test]
    fn k3d_download_url_x86_64() {
        let url = k3d_download_url("x86_64");
        assert!(url.contains("k3d-windows-amd64.exe"));
    }

    #[test]
    fn k3d_download_url_aarch64() {
        let url = k3d_download_url("aarch64");
        assert!(url.contains("k3d-windows-arm64.exe"));
    }

    #[test]
    fn shell_path_converts_backslashes() {
        let path = std::path::PathBuf::from("C:\\Users\\Greg\\file.zip");
        assert_eq!(shell_path(&path), "C:/Users/Greg/file.zip");
    }

    #[test]
    fn shell_path_preserves_forward_slashes() {
        let path = std::path::PathBuf::from("/tmp/file.zip");
        assert_eq!(shell_path(&path), "/tmp/file.zip");
    }
}
