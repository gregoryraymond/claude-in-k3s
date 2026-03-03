use crate::docker::DockerBuilder;
use crate::helm::HelmRunner;
use crate::kubectl::KubectlRunner;

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
    pub memory_usage_mb: Option<u64>,
    pub memory_limit_mb: Option<u64>,
}

impl Default for HealthReport {
    fn default() -> Self {
        Self {
            docker: ComponentHealth::Unknown,
            cluster: ComponentHealth::Unknown,
            node: ComponentHealth::Unknown,
            helm_release: ComponentHealth::Unknown,
            pods: ComponentHealth::Unknown,
            memory_usage_mb: None,
            memory_limit_mb: None,
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

pub async fn check_docker(docker: &DockerBuilder) -> ComponentHealth {
    if docker.is_running().await {
        ComponentHealth::Healthy
    } else {
        ComponentHealth::Unhealthy
    }
}

pub async fn check_cluster(kubectl: &KubectlRunner) -> ComponentHealth {
    match kubectl.cluster_health().await {
        Ok(true) => ComponentHealth::Healthy,
        Ok(false) => ComponentHealth::Unhealthy,
        Err(_) => ComponentHealth::Unhealthy,
    }
}

pub async fn check_node(kubectl: &KubectlRunner) -> ComponentHealth {
    match kubectl.get_nodes().await {
        Ok(nodes) if nodes.is_empty() => ComponentHealth::Unhealthy,
        Ok(nodes) => {
            let all_ready = nodes.iter().all(|n| n.ready);
            if all_ready {
                ComponentHealth::Healthy
            } else {
                ComponentHealth::Degraded
            }
        }
        Err(_) => ComponentHealth::Unknown,
    }
}

pub async fn check_helm(helm: &HelmRunner) -> ComponentHealth {
    match helm.status().await {
        Ok(r) if r.success => ComponentHealth::Healthy,
        Ok(_) => ComponentHealth::Unhealthy,
        Err(_) => ComponentHealth::Unknown,
    }
}

pub async fn check_pods(kubectl: &KubectlRunner) -> (ComponentHealth, Vec<crate::kubectl::PodStatus>) {
    match kubectl.get_pods().await {
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
    }
}

pub async fn full_check(
    docker: &DockerBuilder,
    kubectl: &KubectlRunner,
    helm: &HelmRunner,
) -> (HealthReport, Vec<crate::kubectl::PodStatus>) {
    let docker_health = check_docker(docker).await;
    let cluster_health = check_cluster(kubectl).await;
    let node_health = check_node(kubectl).await;
    let helm_health = check_helm(helm).await;
    let (pods_health, pods) = check_pods(kubectl).await;

    let report = HealthReport {
        docker: docker_health,
        cluster: cluster_health,
        node: node_health,
        helm_release: helm_health,
        pods: pods_health,
        memory_usage_mb: None,
        memory_limit_mb: None,
    };

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
}
