use crate::error::{AppResult, CmdResult};

/// Maximum number of recovery attempts per operation.
const MAX_RECOVERY_ATTEMPTS: u32 = 2;

/// Identifies which recovery action to take based on error output.
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryAction {
    /// Fix Helm namespace ownership labels and retry.
    FixNamespaceOwnership,
    /// Recreate the k3d cluster.
    RecreateK3dCluster,
    /// Delete a crash-looping pod.
    DeletePod(String),
    /// Re-import an image and restart the pod.
    ReimportImage { image: String, pod: String },
}

impl RecoveryAction {
    pub fn description(&self) -> String {
        match self {
            RecoveryAction::FixNamespaceOwnership => {
                "[Recovery] Fixing namespace ownership labels...".to_string()
            }
            RecoveryAction::RecreateK3dCluster => {
                "[Recovery] Recreating k3d cluster...".to_string()
            }
            RecoveryAction::DeletePod(name) => {
                format!("[Recovery] Deleting crash-looping pod {}...", name)
            }
            RecoveryAction::ReimportImage { image, pod } => {
                format!("[Recovery] Re-importing image {} for pod {}...", image, pod)
            }
        }
    }
}

/// Analyze stderr output to detect a known recoverable failure pattern.
pub fn diagnose_helm_failure(stderr: &str) -> Option<RecoveryAction> {
    if stderr.contains("invalid ownership metadata")
        || (stderr.contains("label validation error")
            && stderr.contains("app.kubernetes.io/managed-by"))
    {
        return Some(RecoveryAction::FixNamespaceOwnership);
    }
    None
}

/// Analyze stderr for k3d/cluster failures.
pub fn diagnose_cluster_failure(stderr: &str) -> Option<RecoveryAction> {
    let stderr_lower = stderr.to_lowercase();
    if stderr_lower.contains("k3d")
        && (stderr_lower.contains("corrupted")
            || stderr_lower.contains("connection refused")
            || stderr_lower.contains("cluster not found")
            || stderr_lower.contains("does not exist"))
    {
        return Some(RecoveryAction::RecreateK3dCluster);
    }
    None
}

/// Analyze pod status for recoverable issues.
pub fn diagnose_pod_issues(
    pods: &[crate::kubectl::PodStatus],
) -> Vec<RecoveryAction> {
    let mut actions = Vec::new();
    for pod in pods {
        if pod.restart_count > 5 && (pod.phase == "Running" || pod.phase == "CrashLoopBackOff") {
            actions.push(RecoveryAction::DeletePod(pod.name.clone()));
        }
    }
    actions
}

/// Analyze pod status for image pull issues.
/// Checks waiting container state for ImagePullBackOff or ErrImageNeverPull.
pub fn diagnose_image_issues(
    pods: &[crate::kubectl::PodStatus],
) -> Vec<RecoveryAction> {
    let mut actions = Vec::new();
    for pod in pods {
        if pod.phase == "Pending" || pod.phase == "ImagePullBackOff" || pod.phase == "ErrImageNeverPull" {
            // Extract image tag from pod name (convention: claude-code-{project}-xxx)
            let image = format!("claude-code-{}:latest", pod.project);
            actions.push(RecoveryAction::ReimportImage {
                image,
                pod: pod.name.clone(),
            });
        }
    }
    actions
}

/// Fix namespace ownership labels so Helm can manage it.
pub async fn fix_namespace_ownership(
    kubectl_binary: &str,
    namespace: &str,
) -> AppResult<CmdResult> {
    use std::process::Stdio;
    use tokio::process::Command;

    // Label the namespace for Helm
    let label_result = Command::new(kubectl_binary)
        .args([
            "label", "namespace", namespace,
            "app.kubernetes.io/managed-by=Helm",
            "--overwrite",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !label_result.status.success() {
        return Ok(CmdResult {
            success: false,
            stdout: String::from_utf8_lossy(&label_result.stdout).into(),
            stderr: String::from_utf8_lossy(&label_result.stderr).into(),
        });
    }

    // Annotate release-name
    let _ = Command::new(kubectl_binary)
        .args([
            "annotate", "namespace", namespace,
            &format!("meta.helm.sh/release-name={}", namespace),
            "--overwrite",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    // Annotate release-namespace
    let output = Command::new(kubectl_binary)
        .args([
            "annotate", "namespace", namespace,
            &format!("meta.helm.sh/release-namespace={}", namespace),
            "--overwrite",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    Ok(CmdResult {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into(),
        stderr: String::from_utf8_lossy(&output.stderr).into(),
    })
}

/// Recreate the k3d cluster. Deletes the existing cluster and creates a new one
/// with the specified memory limit.
pub async fn recreate_k3d_cluster(memory_limit: &str) -> AppResult<CmdResult> {
    use std::process::Stdio;
    use tokio::process::Command;

    // Force delete existing cluster
    let _ = Command::new("k3d")
        .args(["cluster", "delete", "claude-code"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;

    // Create new cluster with memory limit
    let output = Command::new("k3d")
        .args([
            "cluster", "create", "claude-code",
            "--wait", "--timeout", "120s",
            "--servers-memory", memory_limit,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    Ok(CmdResult {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into(),
        stderr: String::from_utf8_lossy(&output.stderr).into(),
    })
}

/// Tracks recovery attempt counts per operation type.
pub struct RecoveryTracker {
    helm_attempts: u32,
    cluster_attempts: u32,
}

impl RecoveryTracker {
    pub fn new() -> Self {
        Self {
            helm_attempts: 0,
            cluster_attempts: 0,
        }
    }

    pub fn can_retry_helm(&self) -> bool {
        self.helm_attempts < MAX_RECOVERY_ATTEMPTS
    }

    pub fn record_helm_attempt(&mut self) {
        self.helm_attempts += 1;
    }

    pub fn can_retry_cluster(&self) -> bool {
        self.cluster_attempts < MAX_RECOVERY_ATTEMPTS
    }

    pub fn record_cluster_attempt(&mut self) {
        self.cluster_attempts += 1;
    }

    pub fn reset(&mut self) {
        self.helm_attempts = 0;
        self.cluster_attempts = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnose_helm_namespace_ownership() {
        let stderr = r#"Error: Unable to continue with install: Namespace "claude-code" in namespace "" exists and cannot be imported into the current release: invalid ownership metadata; label validation error: missing key "app.kubernetes.io/managed-by": must be set to "Helm""#;
        let action = diagnose_helm_failure(stderr);
        assert_eq!(action, Some(RecoveryAction::FixNamespaceOwnership));
    }

    #[test]
    fn diagnose_helm_unrelated_error() {
        let stderr = "Error: chart not found";
        let action = diagnose_helm_failure(stderr);
        assert_eq!(action, None);
    }

    #[test]
    fn diagnose_cluster_corrupted() {
        let stderr = "k3d: cluster config corrupted, cannot read state";
        let action = diagnose_cluster_failure(stderr);
        assert_eq!(action, Some(RecoveryAction::RecreateK3dCluster));
    }

    #[test]
    fn diagnose_cluster_connection_refused() {
        let stderr = "k3d: connection refused to cluster API server";
        let action = diagnose_cluster_failure(stderr);
        assert_eq!(action, Some(RecoveryAction::RecreateK3dCluster));
    }

    #[test]
    fn diagnose_cluster_unrelated() {
        let stderr = "kubectl: permission denied";
        let action = diagnose_cluster_failure(stderr);
        assert_eq!(action, None);
    }

    #[test]
    fn diagnose_pods_crash_loop() {
        let pods = vec![
            crate::kubectl::PodStatus {
                name: "pod-1".into(),
                project: "proj".into(),
                phase: "Running".into(),
                ready: true,
                restart_count: 10,
                age: "1h".into(),
                warnings: vec![],
            },
            crate::kubectl::PodStatus {
                name: "pod-2".into(),
                project: "proj".into(),
                phase: "Running".into(),
                ready: true,
                restart_count: 2,
                age: "1h".into(),
                warnings: vec![],
            },
        ];
        let actions = diagnose_pod_issues(&pods);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], RecoveryAction::DeletePod("pod-1".into()));
    }

    #[test]
    fn diagnose_pods_no_issues() {
        let pods = vec![crate::kubectl::PodStatus {
            name: "pod-1".into(),
            project: "proj".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 0,
            age: "1h".into(),
            warnings: vec![],
        }];
        let actions = diagnose_pod_issues(&pods);
        assert!(actions.is_empty());
    }

    #[test]
    fn recovery_tracker_limits() {
        let mut tracker = RecoveryTracker::new();
        assert!(tracker.can_retry_helm());
        tracker.record_helm_attempt();
        assert!(tracker.can_retry_helm());
        tracker.record_helm_attempt();
        assert!(!tracker.can_retry_helm());
    }

    #[test]
    fn recovery_tracker_reset() {
        let mut tracker = RecoveryTracker::new();
        tracker.record_helm_attempt();
        tracker.record_helm_attempt();
        assert!(!tracker.can_retry_helm());
        tracker.reset();
        assert!(tracker.can_retry_helm());
    }

    #[test]
    fn diagnose_image_pull_backoff() {
        let pods = vec![
            crate::kubectl::PodStatus {
                name: "claude-code-myproj-abc123".into(),
                project: "myproj".into(),
                phase: "ImagePullBackOff".into(),
                ready: false,
                restart_count: 0,
                age: "5m".into(),
                warnings: vec![],
            },
            crate::kubectl::PodStatus {
                name: "claude-code-other-def456".into(),
                project: "other".into(),
                phase: "Running".into(),
                ready: true,
                restart_count: 0,
                age: "1h".into(),
                warnings: vec![],
            },
        ];
        let actions = diagnose_image_issues(&pods);
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0],
            RecoveryAction::ReimportImage {
                image: "claude-code-myproj:latest".into(),
                pod: "claude-code-myproj-abc123".into(),
            }
        );
    }

    #[test]
    fn diagnose_image_no_issues() {
        let pods = vec![crate::kubectl::PodStatus {
            name: "pod-1".into(),
            project: "proj".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 0,
            age: "1h".into(),
            warnings: vec![],
        }];
        let actions = diagnose_image_issues(&pods);
        assert!(actions.is_empty());
    }

    #[test]
    fn recovery_action_descriptions() {
        let a = RecoveryAction::FixNamespaceOwnership;
        assert!(a.description().contains("namespace ownership"));

        let b = RecoveryAction::RecreateK3dCluster;
        assert!(b.description().contains("k3d cluster"));

        let c = RecoveryAction::DeletePod("my-pod".into());
        assert!(c.description().contains("my-pod"));
    }
}
