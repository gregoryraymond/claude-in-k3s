use crate::error::{AppResult, CmdResult};

/// Maximum number of recovery attempts per operation.
const MAX_RECOVERY_ATTEMPTS: u32 = 2;

/// Identifies which recovery action to take based on error output.
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryAction {
    /// Fix Helm namespace ownership labels and retry.
    FixNamespaceOwnership,
    /// Clean stale Helm release secrets so a fresh install can proceed.
    CleanHelmRelease,
    /// Uninstall Helm release then retry install.
    ForceReinstallHelm,
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
            RecoveryAction::CleanHelmRelease => {
                "[Recovery] Cleaning stale Helm release state...".to_string()
            }
            RecoveryAction::ForceReinstallHelm => {
                "[Recovery] Uninstalling and reinstalling Helm release...".to_string()
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

// =============================================================================
// Helm failure diagnosis
// =============================================================================

/// Analyze stderr output from a failed `helm upgrade --install` to detect
/// a known recoverable failure pattern.
pub fn diagnose_helm_failure(stderr: &str) -> Option<RecoveryAction> {
    let lower = stderr.to_lowercase();

    // --- Namespace ownership / already-exists issues ---
    // "invalid ownership metadata; label validation error"
    // 'namespaces "claude-code" already exists'
    // "cannot re-use a name" when namespace conflicts
    if stderr.contains("invalid ownership metadata")
        || (stderr.contains("label validation error")
            && stderr.contains("app.kubernetes.io/managed-by"))
        || (lower.contains("already exists") && lower.contains("namespace"))
    {
        return Some(RecoveryAction::FixNamespaceOwnership);
    }

    // --- Stale Helm release / concurrent operation ---
    // "another operation (install/upgrade/rollback) is in progress"
    if lower.contains("another operation") && lower.contains("in progress") {
        return Some(RecoveryAction::CleanHelmRelease);
    }

    // --- Broken release state ---
    // "has no deployed releases" (previous install failed partway)
    // "cannot re-use a name that is still in use"
    // "release: not found" combined with resource conflicts
    // "current release manifest contains removed kubernetes api(s)"
    if lower.contains("has no deployed releases")
        || (lower.contains("cannot re-use") && lower.contains("still in use"))
        || lower.contains("removed kubernetes api")
    {
        return Some(RecoveryAction::ForceReinstallHelm);
    }

    // --- Resource conflicts (non-namespace) ---
    // 'existing resource conflict: ... "some-configmap" already exists'
    // "rendered manifests contain a resource that already exists"
    if lower.contains("resource conflict") || lower.contains("resource that already exists") {
        return Some(RecoveryAction::ForceReinstallHelm);
    }

    // --- Timeout / transient ---
    // "timed out waiting for the condition"
    // "context deadline exceeded"
    // These are worth a plain retry — map to CleanHelmRelease which
    // clears any partial state then the caller retries.
    if lower.contains("timed out waiting") || lower.contains("context deadline exceeded") {
        return Some(RecoveryAction::CleanHelmRelease);
    }

    // --- RBAC / permission issues on install ---
    // "forbidden: User ... cannot create resource"
    // Not auto-recoverable but we can try force reinstall to reset state
    if lower.contains("forbidden") && lower.contains("cannot") {
        return Some(RecoveryAction::ForceReinstallHelm);
    }

    None
}

// =============================================================================
// Cluster failure diagnosis
// =============================================================================

/// Analyze stderr for k3d/cluster failures.
pub fn diagnose_cluster_failure(stderr: &str) -> Option<RecoveryAction> {
    let lower = stderr.to_lowercase();

    // k3d-specific errors
    if lower.contains("k3d")
        && (lower.contains("corrupted")
            || lower.contains("connection refused")
            || lower.contains("cluster not found")
            || lower.contains("does not exist"))
    {
        return Some(RecoveryAction::RecreateK3dCluster);
    }

    // Generic cluster connectivity issues
    // "Unable to connect to the server"
    // "dial tcp ... connect: connection refused"
    // "the server has asked for the client to provide credentials"
    // "certificate has expired or is not yet valid"
    // "TLS handshake timeout"
    // "was refused - did you specify the right host or port?"
    if lower.contains("unable to connect to the server")
        || (lower.contains("dial tcp") && lower.contains("connection refused"))
        || lower.contains("the server has asked for the client to provide credentials")
        || lower.contains("certificate has expired")
        || lower.contains("certificate is not yet valid")
        || lower.contains("tls handshake timeout")
        || lower.contains("did you specify the right host or port")
    {
        return Some(RecoveryAction::RecreateK3dCluster);
    }

    // etcd / API server issues that indicate a broken cluster
    // "etcdserver: leader changed"
    // "etcdserver: request timed out"
    // "the object has been modified; please apply your changes to the latest version"
    if lower.contains("etcdserver:") && (lower.contains("leader changed") || lower.contains("request timed out")) {
        return Some(RecoveryAction::RecreateK3dCluster);
    }

    None
}

// =============================================================================
// Pod issue diagnosis
// =============================================================================

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

// =============================================================================
// Recovery actions
// =============================================================================

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

/// Clean stale Helm release secrets in a namespace.
/// This removes the release state that blocks a new install/upgrade.
pub async fn clean_helm_release(
    kubectl_binary: &str,
    namespace: &str,
) -> AppResult<CmdResult> {
    use std::process::Stdio;
    use tokio::process::Command;

    let output = Command::new(kubectl_binary)
        .args([
            "delete", "secrets",
            "-n", namespace,
            "-l", "owner=helm",
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

/// Force-uninstall a Helm release, ignoring errors (the release may not exist).
pub async fn force_uninstall_helm(
    helm_binary: &str,
    release_name: &str,
    namespace: &str,
) -> AppResult<CmdResult> {
    use std::process::Stdio;
    use tokio::process::Command;

    // Try normal uninstall first
    let output = Command::new(helm_binary)
        .args([
            "uninstall", release_name,
            "--namespace", namespace,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    // If normal uninstall fails, clean up secrets directly
    if !output.status.success() {
        let kubectl = if helm_binary.ends_with(".exe") {
            "kubectl.exe"
        } else {
            "kubectl"
        };
        return clean_helm_release(kubectl, namespace).await;
    }

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

    // =========================================================================
    // Helm failure diagnosis
    // =========================================================================

    #[test]
    fn helm_invalid_ownership_metadata() {
        let stderr = r#"Error: Unable to continue with install: Namespace "claude-code" in namespace "" exists and cannot be imported into the current release: invalid ownership metadata; label validation error: missing key "app.kubernetes.io/managed-by": must be set to "Helm""#;
        assert_eq!(diagnose_helm_failure(stderr), Some(RecoveryAction::FixNamespaceOwnership));
    }

    #[test]
    fn helm_namespace_already_exists() {
        let stderr = r#"Error: 1 error occurred:
    * namespaces "claude-code" already exists"#;
        assert_eq!(diagnose_helm_failure(stderr), Some(RecoveryAction::FixNamespaceOwnership));
    }

    #[test]
    fn helm_another_operation_in_progress() {
        let stderr = "Error: UPGRADE FAILED: another operation (install/upgrade/rollback) is in progress";
        assert_eq!(diagnose_helm_failure(stderr), Some(RecoveryAction::CleanHelmRelease));
    }

    #[test]
    fn helm_no_deployed_releases() {
        let stderr = r#"Error: UPGRADE FAILED: "claude-code" has no deployed releases"#;
        assert_eq!(diagnose_helm_failure(stderr), Some(RecoveryAction::ForceReinstallHelm));
    }

    #[test]
    fn helm_cannot_reuse_name() {
        let stderr = "Error: cannot re-use a name that is still in use";
        assert_eq!(diagnose_helm_failure(stderr), Some(RecoveryAction::ForceReinstallHelm));
    }

    #[test]
    fn helm_resource_already_exists() {
        let stderr = "Error: rendered manifests contain a resource that already exists. Unable to continue with install";
        assert_eq!(diagnose_helm_failure(stderr), Some(RecoveryAction::ForceReinstallHelm));
    }

    #[test]
    fn helm_resource_conflict() {
        let stderr = r#"Error: existing resource conflict: namespace: claude-code, name: claude-code-config, existing_kind: /v1, Kind=ConfigMap, new_kind: /v1, Kind=ConfigMap"#;
        assert_eq!(diagnose_helm_failure(stderr), Some(RecoveryAction::ForceReinstallHelm));
    }

    #[test]
    fn helm_timed_out() {
        let stderr = "Error: timed out waiting for the condition";
        assert_eq!(diagnose_helm_failure(stderr), Some(RecoveryAction::CleanHelmRelease));
    }

    #[test]
    fn helm_context_deadline() {
        let stderr = "Error: INSTALLATION FAILED: context deadline exceeded";
        assert_eq!(diagnose_helm_failure(stderr), Some(RecoveryAction::CleanHelmRelease));
    }

    #[test]
    fn helm_forbidden() {
        let stderr = r#"Error: INSTALLATION FAILED: forbidden: User "system:serviceaccount:default:default" cannot create resource "deployments" in API group "apps" in the namespace "claude-code""#;
        assert_eq!(diagnose_helm_failure(stderr), Some(RecoveryAction::ForceReinstallHelm));
    }

    #[test]
    fn helm_removed_kubernetes_api() {
        let stderr = "Error: UPGRADE FAILED: current release manifest contains removed kubernetes api(s) for this kubernetes version";
        assert_eq!(diagnose_helm_failure(stderr), Some(RecoveryAction::ForceReinstallHelm));
    }

    #[test]
    fn helm_unrelated_error() {
        let stderr = "Error: chart not found";
        assert_eq!(diagnose_helm_failure(stderr), None);
    }

    // =========================================================================
    // Cluster failure diagnosis
    // =========================================================================

    #[test]
    fn cluster_corrupted() {
        let stderr = "k3d: cluster config corrupted, cannot read state";
        assert_eq!(diagnose_cluster_failure(stderr), Some(RecoveryAction::RecreateK3dCluster));
    }

    #[test]
    fn cluster_connection_refused() {
        let stderr = "k3d: connection refused to cluster API server";
        assert_eq!(diagnose_cluster_failure(stderr), Some(RecoveryAction::RecreateK3dCluster));
    }

    #[test]
    fn cluster_unable_to_connect() {
        let stderr = "Unable to connect to the server: dial tcp 127.0.0.1:6443: connect: connection refused";
        assert_eq!(diagnose_cluster_failure(stderr), Some(RecoveryAction::RecreateK3dCluster));
    }

    #[test]
    fn cluster_certificate_expired() {
        let stderr = "Unable to connect to the server: x509: certificate has expired or is not yet valid";
        assert_eq!(diagnose_cluster_failure(stderr), Some(RecoveryAction::RecreateK3dCluster));
    }

    #[test]
    fn cluster_credentials_required() {
        let stderr = "error: the server has asked for the client to provide credentials";
        assert_eq!(diagnose_cluster_failure(stderr), Some(RecoveryAction::RecreateK3dCluster));
    }

    #[test]
    fn cluster_wrong_host_or_port() {
        let stderr = "The connection to the server localhost:6443 was refused - did you specify the right host or port?";
        assert_eq!(diagnose_cluster_failure(stderr), Some(RecoveryAction::RecreateK3dCluster));
    }

    #[test]
    fn cluster_tls_handshake_timeout() {
        let stderr = "Unable to connect to the server: net/http: TLS handshake timeout";
        assert_eq!(diagnose_cluster_failure(stderr), Some(RecoveryAction::RecreateK3dCluster));
    }

    #[test]
    fn cluster_dial_tcp_refused() {
        let stderr = "dial tcp 192.168.1.100:6443: connect: connection refused";
        assert_eq!(diagnose_cluster_failure(stderr), Some(RecoveryAction::RecreateK3dCluster));
    }

    #[test]
    fn cluster_etcd_leader_changed() {
        let stderr = "etcdserver: leader changed";
        assert_eq!(diagnose_cluster_failure(stderr), Some(RecoveryAction::RecreateK3dCluster));
    }

    #[test]
    fn cluster_etcd_request_timed_out() {
        let stderr = "etcdserver: request timed out";
        assert_eq!(diagnose_cluster_failure(stderr), Some(RecoveryAction::RecreateK3dCluster));
    }

    #[test]
    fn cluster_unrelated() {
        let stderr = "kubectl: permission denied";
        assert_eq!(diagnose_cluster_failure(stderr), None);
    }

    // =========================================================================
    // Pod / image diagnosis
    // =========================================================================

    #[test]
    fn pods_crash_loop() {
        let pods = vec![
            crate::kubectl::PodStatus {
                name: "pod-1".into(),
                project: "proj".into(),
                phase: "Running".into(),
                ready: true,
                restart_count: 10,
                age: "1h".into(),
                warnings: vec![],
                exposed: false,
                container_port: 0,
                selected: false,
            },
            crate::kubectl::PodStatus {
                name: "pod-2".into(),
                project: "proj".into(),
                phase: "Running".into(),
                ready: true,
                restart_count: 2,
                age: "1h".into(),
                warnings: vec![],
                exposed: false,
                container_port: 0,
                selected: false,
            },
        ];
        let actions = diagnose_pod_issues(&pods);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], RecoveryAction::DeletePod("pod-1".into()));
    }

    #[test]
    fn pods_no_issues() {
        let pods = vec![crate::kubectl::PodStatus {
            name: "pod-1".into(),
            project: "proj".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 0,
            age: "1h".into(),
            warnings: vec![],
            exposed: false,
            container_port: 0,
            selected: false,
        }];
        let actions = diagnose_pod_issues(&pods);
        assert!(actions.is_empty());
    }

    #[test]
    fn image_pull_backoff() {
        let pods = vec![
            crate::kubectl::PodStatus {
                name: "claude-code-myproj-abc123".into(),
                project: "myproj".into(),
                phase: "ImagePullBackOff".into(),
                ready: false,
                restart_count: 0,
                age: "5m".into(),
                warnings: vec![],
                exposed: false,
                container_port: 0,
                selected: false,
            },
            crate::kubectl::PodStatus {
                name: "claude-code-other-def456".into(),
                project: "other".into(),
                phase: "Running".into(),
                ready: true,
                restart_count: 0,
                age: "1h".into(),
                warnings: vec![],
                exposed: false,
                container_port: 0,
                selected: false,
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
    fn image_no_issues() {
        let pods = vec![crate::kubectl::PodStatus {
            name: "pod-1".into(),
            project: "proj".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 0,
            age: "1h".into(),
            warnings: vec![],
            exposed: false,
            container_port: 0,
            selected: false,
        }];
        let actions = diagnose_image_issues(&pods);
        assert!(actions.is_empty());
    }

    // =========================================================================
    // Recovery tracker
    // =========================================================================

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
    fn recovery_action_descriptions() {
        assert!(RecoveryAction::FixNamespaceOwnership.description().contains("namespace ownership"));
        assert!(RecoveryAction::CleanHelmRelease.description().contains("stale"));
        assert!(RecoveryAction::ForceReinstallHelm.description().contains("reinstalling"));
        assert!(RecoveryAction::RecreateK3dCluster.description().contains("k3d cluster"));
        assert!(RecoveryAction::DeletePod("my-pod".into()).description().contains("my-pod"));
    }
}
