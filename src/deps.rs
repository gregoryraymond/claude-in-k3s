use crate::platform::{self, Platform};
use std::process::Command;

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
        k3s: check_tool("k3s"),
        terraform: check_tool(platform::terraform_binary(platform)),
        helm: check_tool(platform::helm_binary(platform)),
        docker: check_tool(platform::docker_binary(platform)),
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
}
