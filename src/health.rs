use crate::docker::DockerBuilder;
use crate::helm::HelmRunner;
use crate::kubectl::KubectlRunner;
use crate::platform::Platform;

#[derive(Debug, Clone, PartialEq)]
pub enum ComponentHealth {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

impl ComponentHealth {
    pub fn as_str(&self) -> &'static str {
        match self {
            ComponentHealth::Healthy => "Healthy",
            ComponentHealth::Degraded => "Degraded",
            ComponentHealth::Unhealthy => "Unhealthy",
            ComponentHealth::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct HealthReport {
    pub docker: ComponentHealth,
    pub cluster: ComponentHealth,
    pub node: ComponentHealth,
    pub helm_release: ComponentHealth,
    pub pods: ComponentHealth,
    /// WSL health (Windows only; `Unknown` on non-Windows platforms).
    pub wsl: ComponentHealth,
    pub memory_usage_mb: Option<u64>,
    pub memory_limit_mb: Option<u64>,
    /// e.g. "v1.31.3+k3s1 — up 3d 2h"
    pub cluster_detail: String,
    /// e.g. "3 releases"
    pub helm_detail: String,
    /// Descriptive error when Docker is unhealthy, e.g. "Docker daemon not running"
    pub docker_detail: String,
}

impl Default for HealthReport {
    fn default() -> Self {
        Self {
            docker: ComponentHealth::Unknown,
            cluster: ComponentHealth::Unknown,
            node: ComponentHealth::Unknown,
            helm_release: ComponentHealth::Unknown,
            pods: ComponentHealth::Unknown,
            wsl: ComponentHealth::Unknown,
            memory_usage_mb: None,
            memory_limit_mb: None,
            cluster_detail: String::new(),
            helm_detail: String::new(),
            docker_detail: String::new(),
        }
    }
}

impl HealthReport {
    pub fn overall(&self) -> ComponentHealth {
        let components = [
            &self.docker,
            &self.cluster,
            &self.node,
            &self.helm_release,
            &self.pods,
        ];
        if components.iter().any(|c| **c == ComponentHealth::Unhealthy) {
            ComponentHealth::Unhealthy
        } else if components.iter().any(|c| **c == ComponentHealth::Degraded) {
            ComponentHealth::Degraded
        } else if components.iter().all(|c| **c == ComponentHealth::Healthy) {
            ComponentHealth::Healthy
        } else {
            ComponentHealth::Unknown
        }
    }

    /// PRJ-30: Check if cluster has capacity for `additional_mb` of memory.
    /// Returns `None` if memory info isn't available, `Some(true)` if there's room,
    /// `Some(false)` if adding the workload would exceed the limit.
    pub fn has_memory_capacity(&self, additional_mb: u64) -> Option<bool> {
        match (self.memory_usage_mb, self.memory_limit_mb) {
            (Some(used), Some(limit)) if limit > 0 => Some(used + additional_mb <= limit),
            _ => None,
        }
    }

    pub fn memory_usage_text(&self) -> String {
        match (self.memory_usage_mb, self.memory_limit_mb) {
            (Some(used), Some(limit)) => {
                let percent = if limit > 0 { used * 100 / limit } else { 0 };
                format!(
                    "Memory: {:.1} GB / {:.1} GB ({}%)",
                    used as f64 / 1024.0,
                    limit as f64 / 1024.0,
                    percent
                )
            }
            _ => "Memory: --".to_string(),
        }
    }
}

/// Check WSL health by running `wsl --status`. Only meaningful on Windows.
pub async fn check_wsl(platform: &Platform) -> ComponentHealth {
    if *platform != Platform::Windows {
        return ComponentHealth::Unknown;
    }

    use std::process::Stdio;
    use tokio::process::Command;

    let result = match Command::new("wsl")
        .args(["--status"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
    {
        Ok(output) if output.status.success() => ComponentHealth::Healthy,
        Ok(_) => ComponentHealth::Unhealthy,
        Err(_) => ComponentHealth::Unhealthy,
    };
    tracing::debug!("WSL health: {:?}", result);
    result
}

pub async fn check_docker(docker: &DockerBuilder) -> (ComponentHealth, String) {
    let (running, detail) = docker.check_health().await;
    let result = if running {
        (ComponentHealth::Healthy, String::new())
    } else {
        (ComponentHealth::Unhealthy, detail)
    };
    tracing::debug!("Docker health: {:?}", result.0);
    result
}

pub async fn check_cluster(kubectl: &KubectlRunner) -> ComponentHealth {
    let result = match kubectl.cluster_health().await {
        Ok(true) => ComponentHealth::Healthy,
        Ok(false) => ComponentHealth::Unhealthy,
        Err(_) => ComponentHealth::Unhealthy,
    };
    tracing::debug!("Cluster health: {:?}", result);
    result
}

/// Returns (health, detail_string, memory_capacity_mb, memory_allocatable_mb).
pub async fn check_node(kubectl: &KubectlRunner) -> (ComponentHealth, String, Option<u64>, Option<u64>) {
    let result = match kubectl.get_nodes().await {
        Ok(nodes) if nodes.is_empty() => (ComponentHealth::Unhealthy, String::new(), None, None),
        Ok(nodes) => {
            let all_ready = nodes.iter().all(|n| n.ready);
            let health = if all_ready {
                ComponentHealth::Healthy
            } else {
                ComponentHealth::Degraded
            };

            // Build detail from first node (single-node cluster typical for k3d/k3s)
            let (detail, cap, alloc) = if let Some(node) = nodes.first() {
                let mut parts = Vec::new();
                if !node.version.is_empty() {
                    parts.push(node.version.clone());
                }
                if !node.age.is_empty() {
                    parts.push(format!("up {}", node.age));
                }
                (parts.join(" - "), node.memory_capacity_mb, node.memory_allocatable_mb)
            } else {
                (String::new(), None, None)
            };

            (health, detail, cap, alloc)
        }
        Err(_) => (ComponentHealth::Unknown, String::new(), None, None),
    };
    tracing::debug!("Node health: {:?}", result.0);
    result
}

pub async fn check_helm(helm: &HelmRunner) -> ComponentHealth {
    let result = match helm.status().await {
        Ok(r) if r.success => ComponentHealth::Healthy,
        Ok(_) => ComponentHealth::Unhealthy,
        Err(_) => ComponentHealth::Unknown,
    };
    tracing::debug!("Helm health: {:?}", result);
    result
}

pub async fn check_pods(kubectl: &KubectlRunner) -> (ComponentHealth, Vec<crate::kubectl::PodStatus>) {
    let result = match kubectl.get_pods().await {
        Ok(pods) if pods.is_empty() => (ComponentHealth::Unknown, pods),
        Ok(pods) => {
            let has_crash = pods.iter().any(|p| {
                p.phase == "Failed" || p.restart_count > 5
            });
            let all_running = pods.iter().all(|p| p.phase == "Running" && p.ready);
            let health = if has_crash {
                ComponentHealth::Unhealthy
            } else if all_running {
                ComponentHealth::Healthy
            } else {
                ComponentHealth::Degraded
            };
            (health, pods)
        }
        Err(_) => (ComponentHealth::Unknown, vec![]),
    };
    tracing::debug!("Pods health: {:?}", result.0);
    result
}

pub async fn full_check(
    docker: &DockerBuilder,
    kubectl: &KubectlRunner,
    helm: &HelmRunner,
    platform: &Platform,
) -> (HealthReport, Vec<crate::kubectl::PodStatus>) {
    let (docker_health, docker_detail) = check_docker(docker).await;
    let cluster_health = check_cluster(kubectl).await;
    let (node_health, cluster_detail, _, _) = check_node(kubectl).await;
    let helm_health = check_helm(helm).await;
    let (pods_health, pods) = check_pods(kubectl).await;
    let wsl_health = check_wsl(platform).await;

    // Get actual memory usage from metrics-server
    let (memory_usage_mb, memory_limit_mb) = match kubectl.top_node_memory().await {
        Some((used, total)) => (Some(used), Some(total)),
        None => (None, None),
    };

    let release_count = helm.release_count().await;
    let helm_detail = if release_count > 0 {
        format!("{} {}", release_count, if release_count == 1 { "release" } else { "releases" })
    } else {
        String::new()
    };

    let report = HealthReport {
        docker: docker_health,
        cluster: cluster_health,
        node: node_health,
        helm_release: helm_health,
        pods: pods_health,
        wsl: wsl_health,
        memory_usage_mb,
        memory_limit_mb,
        cluster_detail,
        helm_detail,
        docker_detail,
    };

    tracing::info!(
        "Full health check: overall={:?}, docker={:?}, cluster={:?}, node={:?}, helm={:?}, pods={:?}",
        report.overall(),
        report.docker,
        report.cluster,
        report.node,
        report.helm_release,
        report.pods,
    );

    (report, pods)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_health_report_all_unknown() {
        let report = HealthReport::default();
        assert_eq!(report.docker, ComponentHealth::Unknown);
        assert_eq!(report.cluster, ComponentHealth::Unknown);
        assert_eq!(report.node, ComponentHealth::Unknown);
        assert_eq!(report.helm_release, ComponentHealth::Unknown);
        assert_eq!(report.pods, ComponentHealth::Unknown);
        assert!(report.memory_usage_mb.is_none());
        assert!(report.memory_limit_mb.is_none());
    }

    #[test]
    fn overall_all_healthy() {
        let report = HealthReport {
            docker: ComponentHealth::Healthy,
            cluster: ComponentHealth::Healthy,
            node: ComponentHealth::Healthy,
            helm_release: ComponentHealth::Healthy,
            pods: ComponentHealth::Healthy,
            memory_usage_mb: None,
            memory_limit_mb: None,
            ..HealthReport::default()
        };
        assert_eq!(report.overall(), ComponentHealth::Healthy);
    }

    #[test]
    fn overall_one_unhealthy() {
        let report = HealthReport {
            docker: ComponentHealth::Healthy,
            cluster: ComponentHealth::Unhealthy,
            node: ComponentHealth::Healthy,
            helm_release: ComponentHealth::Healthy,
            pods: ComponentHealth::Healthy,
            memory_usage_mb: None,
            memory_limit_mb: None,
            ..HealthReport::default()
        };
        assert_eq!(report.overall(), ComponentHealth::Unhealthy);
    }

    #[test]
    fn overall_one_degraded() {
        let report = HealthReport {
            docker: ComponentHealth::Healthy,
            cluster: ComponentHealth::Healthy,
            node: ComponentHealth::Degraded,
            helm_release: ComponentHealth::Healthy,
            pods: ComponentHealth::Healthy,
            memory_usage_mb: None,
            memory_limit_mb: None,
            ..HealthReport::default()
        };
        assert_eq!(report.overall(), ComponentHealth::Degraded);
    }

    #[test]
    fn overall_mixed_unknown_and_healthy() {
        let report = HealthReport {
            docker: ComponentHealth::Healthy,
            cluster: ComponentHealth::Healthy,
            node: ComponentHealth::Unknown,
            helm_release: ComponentHealth::Healthy,
            pods: ComponentHealth::Healthy,
            memory_usage_mb: None,
            memory_limit_mb: None,
            ..HealthReport::default()
        };
        assert_eq!(report.overall(), ComponentHealth::Unknown);
    }

    #[test]
    fn memory_usage_text_with_values() {
        let report = HealthReport {
            memory_usage_mb: Some(6348),
            memory_limit_mb: Some(12800),
            ..HealthReport::default()
        };
        let text = report.memory_usage_text();
        assert!(text.contains("6.2 GB"), "got: {}", text);
        assert!(text.contains("12.5 GB"), "got: {}", text);
        assert!(text.contains("49%"), "got: {}", text);
    }

    #[test]
    fn memory_usage_text_without_values() {
        let report = HealthReport::default();
        assert_eq!(report.memory_usage_text(), "Memory: --");
    }

    #[test]
    fn component_health_as_str() {
        assert_eq!(ComponentHealth::Healthy.as_str(), "Healthy");
        assert_eq!(ComponentHealth::Degraded.as_str(), "Degraded");
        assert_eq!(ComponentHealth::Unhealthy.as_str(), "Unhealthy");
        assert_eq!(ComponentHealth::Unknown.as_str(), "Unknown");
    }

    // =========================================================================
    // Overall health priority edge cases
    // =========================================================================

    #[test]
    fn overall_unhealthy_takes_priority_over_degraded() {
        let report = HealthReport {
            docker: ComponentHealth::Degraded,
            cluster: ComponentHealth::Unhealthy,
            node: ComponentHealth::Healthy,
            helm_release: ComponentHealth::Healthy,
            pods: ComponentHealth::Degraded,
            memory_usage_mb: None,
            memory_limit_mb: None,
            ..HealthReport::default()
        };
        assert_eq!(report.overall(), ComponentHealth::Unhealthy);
    }

    #[test]
    fn overall_all_unknown() {
        let report = HealthReport::default();
        assert_eq!(report.overall(), ComponentHealth::Unknown);
    }

    #[test]
    fn overall_degraded_and_unknown_mixed() {
        let report = HealthReport {
            docker: ComponentHealth::Healthy,
            cluster: ComponentHealth::Healthy,
            node: ComponentHealth::Degraded,
            helm_release: ComponentHealth::Unknown,
            pods: ComponentHealth::Healthy,
            ..HealthReport::default()
        };
        // Degraded takes priority over Unknown (checked before Unknown falls through)
        assert_eq!(report.overall(), ComponentHealth::Degraded);
    }

    #[test]
    fn overall_single_unknown_prevents_healthy() {
        // Even if 4/5 are healthy, one Unknown makes it Unknown (not Healthy)
        let report = HealthReport {
            docker: ComponentHealth::Healthy,
            cluster: ComponentHealth::Healthy,
            node: ComponentHealth::Healthy,
            helm_release: ComponentHealth::Healthy,
            pods: ComponentHealth::Unknown,
            memory_usage_mb: None,
            memory_limit_mb: None,
            ..HealthReport::default()
        };
        assert_eq!(report.overall(), ComponentHealth::Unknown);
    }

    // =========================================================================
    // Memory usage edge cases
    // =========================================================================

    #[test]
    fn memory_usage_text_zero_limit() {
        let report = HealthReport {
            memory_usage_mb: Some(100),
            memory_limit_mb: Some(0),
            ..HealthReport::default()
        };
        let text = report.memory_usage_text();
        assert!(text.contains("0%"), "zero limit should show 0%, got: {}", text);
    }

    #[test]
    fn memory_usage_text_zero_usage() {
        let report = HealthReport {
            memory_usage_mb: Some(0),
            memory_limit_mb: Some(8192),
            ..HealthReport::default()
        };
        let text = report.memory_usage_text();
        assert!(text.contains("0.0 GB"), "zero usage, got: {}", text);
        assert!(text.contains("0%"), "zero usage, got: {}", text);
    }

    #[test]
    fn memory_usage_text_only_usage_no_limit() {
        let report = HealthReport {
            memory_usage_mb: Some(4096),
            memory_limit_mb: None,
            ..HealthReport::default()
        };
        assert_eq!(report.memory_usage_text(), "Memory: --");
    }

    #[test]
    fn memory_usage_text_only_limit_no_usage() {
        let report = HealthReport {
            memory_usage_mb: None,
            memory_limit_mb: Some(8192),
            ..HealthReport::default()
        };
        assert_eq!(report.memory_usage_text(), "Memory: --");
    }

    #[test]
    fn memory_usage_text_large_values() {
        // 64 GB system, 48 GB used
        let report = HealthReport {
            memory_usage_mb: Some(49152),
            memory_limit_mb: Some(65536),
            ..HealthReport::default()
        };
        let text = report.memory_usage_text();
        assert!(text.contains("48.0 GB"), "got: {}", text);
        assert!(text.contains("64.0 GB"), "got: {}", text);
        assert!(text.contains("75%"), "got: {}", text);
    }

    #[test]
    fn memory_usage_text_small_values() {
        // 512 MB used of 2048 MB
        let report = HealthReport {
            memory_usage_mb: Some(512),
            memory_limit_mb: Some(2048),
            ..HealthReport::default()
        };
        let text = report.memory_usage_text();
        assert!(text.contains("0.5 GB"), "got: {}", text);
        assert!(text.contains("2.0 GB"), "got: {}", text);
        assert!(text.contains("25%"), "got: {}", text);
    }

    // =========================================================================
    // HealthReport construction and field access
    // =========================================================================

    #[test]
    fn health_report_custom_details() {
        let report = HealthReport {
            docker: ComponentHealth::Healthy,
            cluster: ComponentHealth::Healthy,
            node: ComponentHealth::Healthy,
            helm_release: ComponentHealth::Healthy,
            pods: ComponentHealth::Healthy,
            wsl: ComponentHealth::Unknown,
            memory_usage_mb: Some(4096),
            memory_limit_mb: Some(8192),
            cluster_detail: "v1.31.3+k3s1 - up 3d 2h".to_string(),
            helm_detail: "3 releases".to_string(),
            docker_detail: String::new(),
        };
        assert_eq!(report.overall(), ComponentHealth::Healthy);
        assert_eq!(report.cluster_detail, "v1.31.3+k3s1 - up 3d 2h");
        assert_eq!(report.helm_detail, "3 releases");
        assert!(report.docker_detail.is_empty());
    }

    #[test]
    fn health_report_docker_unhealthy_detail() {
        let report = HealthReport {
            docker: ComponentHealth::Unhealthy,
            docker_detail: "Docker daemon not running".to_string(),
            ..HealthReport::default()
        };
        assert_eq!(report.overall(), ComponentHealth::Unhealthy);
        assert_eq!(report.docker_detail, "Docker daemon not running");
    }

    // =========================================================================
    // WSL health field tests
    // =========================================================================

    #[test]
    fn default_health_report_wsl_unknown() {
        let report = HealthReport::default();
        assert_eq!(report.wsl, ComponentHealth::Unknown);
    }

    #[test]
    fn overall_ignores_wsl_field() {
        // WSL is platform-specific and should not affect overall()
        let report = HealthReport {
            docker: ComponentHealth::Healthy,
            cluster: ComponentHealth::Healthy,
            node: ComponentHealth::Healthy,
            helm_release: ComponentHealth::Healthy,
            pods: ComponentHealth::Healthy,
            wsl: ComponentHealth::Unhealthy, // should not drag overall down
            ..HealthReport::default()
        };
        assert_eq!(report.overall(), ComponentHealth::Healthy);
    }

    #[tokio::test]
    async fn check_wsl_returns_unknown_on_non_windows() {
        let result = check_wsl(&Platform::Linux).await;
        assert_eq!(result, ComponentHealth::Unknown);

        let result2 = check_wsl(&Platform::MacOs).await;
        assert_eq!(result2, ComponentHealth::Unknown);
    }

    #[test]
    fn memory_usage_text_full_usage() {
        let report = HealthReport {
            memory_usage_mb: Some(8192),
            memory_limit_mb: Some(8192),
            ..HealthReport::default()
        };
        let text = report.memory_usage_text();
        assert!(text.contains("100%"), "full usage should show 100%, got: {}", text);
    }

    // =========================================================================
    // PRJ-30: has_memory_capacity
    // =========================================================================

    #[test]
    fn has_memory_capacity_with_room() {
        let report = HealthReport {
            memory_usage_mb: Some(2048),
            memory_limit_mb: Some(8192),
            ..HealthReport::default()
        };
        assert_eq!(report.has_memory_capacity(4096), Some(true));
    }

    #[test]
    fn has_memory_capacity_at_limit() {
        let report = HealthReport {
            memory_usage_mb: Some(4096),
            memory_limit_mb: Some(8192),
            ..HealthReport::default()
        };
        assert_eq!(report.has_memory_capacity(4096), Some(true));
    }

    #[test]
    fn has_memory_capacity_over_limit() {
        let report = HealthReport {
            memory_usage_mb: Some(6000),
            memory_limit_mb: Some(8192),
            ..HealthReport::default()
        };
        assert_eq!(report.has_memory_capacity(4096), Some(false));
    }

    #[test]
    fn has_memory_capacity_unknown() {
        let report = HealthReport::default();
        assert_eq!(report.has_memory_capacity(4096), None);
    }
}
