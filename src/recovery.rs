#![allow(dead_code)]
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

    /// User-facing manual fix steps shown when auto-recovery fails.
    pub fn manual_steps(&self) -> &'static str {
        match self {
            RecoveryAction::FixNamespaceOwnership => {
                "Manual fix: Run 'kubectl delete namespace claude-code' then redeploy from the Cluster panel."
            }
            RecoveryAction::CleanHelmRelease => {
                "Manual fix: Run 'helm list -A' to find stale releases, then 'helm uninstall <release> -n <namespace>'. Redeploy afterward."
            }
            RecoveryAction::ForceReinstallHelm => {
                "Manual fix: Run 'helm uninstall claude-<project> -n claude-code', then redeploy from the Projects panel."
            }
            RecoveryAction::RecreateK3dCluster => {
                "Manual fix: Run 'k3d cluster delete claude-cluster && k3d cluster create claude-cluster'. Then redeploy."
            }
            RecoveryAction::DeletePod(_) => {
                "Manual fix: Run 'kubectl delete pod <pod-name> -n claude-code'. The Deployment will recreate it."
            }
            RecoveryAction::ReimportImage { .. } => {
                "Manual fix: Rebuild the image from the Projects panel, or run 'k3d image import <image> -c claude-cluster'."
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
// Terraform failure diagnosis (CLU-33)
// =============================================================================

/// Returns `true` if the Terraform error output indicates state corruption
/// that can be fixed with `terraform init -reconfigure`.
pub fn is_terraform_state_corrupt(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();

    // Backend configuration changed
    lower.contains("backend configuration changed")
        // State file version mismatch
        || lower.contains("state snapshot was created by terraform")
        // Provider/plugin issues requiring reinit
        || lower.contains("required plugins are not installed")
        || lower.contains("provider registry")
        // Lock file issues
        || lower.contains("dependency lock file")
        // State is malformed
        || lower.contains("error loading state")
        || lower.contains("failed to decode current backend config")
        || lower.contains("unsupported state file format")
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

    tracing::info!("Attempting recovery: fix namespace ownership for '{}'", namespace);

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
        let stderr = String::from_utf8_lossy(&label_result.stderr).to_string();
        tracing::warn!("Recovery failed: fix namespace ownership label: {}", stderr);
        return Ok(CmdResult {
            success: false,
            stdout: String::from_utf8_lossy(&label_result.stdout).into(),
            stderr,
        });
    }

    // Annotate release-name
    match Command::new(kubectl_binary)
        .args([
            "annotate", "namespace", namespace,
            &format!("meta.helm.sh/release-name={}", namespace),
            "--overwrite",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
    {
        Ok(output) if !output.status.success() => {
            tracing::warn!("Failed to annotate release-name on namespace '{}': {}", namespace, String::from_utf8_lossy(&output.stderr));
        }
        Err(e) => {
            tracing::warn!("Failed to annotate release-name on namespace '{}': {}", namespace, e);
        }
        _ => {}
    }

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

    let result = CmdResult {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into(),
        stderr: String::from_utf8_lossy(&output.stderr).into(),
    };
    if result.success {
        tracing::info!("Recovery succeeded: fix namespace ownership for '{}'", namespace);
    } else {
        tracing::warn!("Recovery failed: fix namespace ownership: {}", result.stderr);
    }
    Ok(result)
}

/// Clean stale Helm release secrets in a namespace.
/// This removes the release state that blocks a new install/upgrade.
pub async fn clean_helm_release(
    kubectl_binary: &str,
    namespace: &str,
) -> AppResult<CmdResult> {
    use std::process::Stdio;
    use tokio::process::Command;

    tracing::info!("Attempting recovery: clean Helm release secrets in '{}'", namespace);

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

    let result = CmdResult {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into(),
        stderr: String::from_utf8_lossy(&output.stderr).into(),
    };
    if result.success {
        tracing::info!("Recovery succeeded: clean Helm release secrets in '{}'", namespace);
    } else {
        tracing::warn!("Recovery failed: clean Helm release secrets: {}", result.stderr);
    }
    Ok(result)
}

/// Force-uninstall a Helm release, ignoring errors (the release may not exist).
pub async fn force_uninstall_helm(
    helm_binary: &str,
    release_name: &str,
    namespace: &str,
) -> AppResult<CmdResult> {
    use std::process::Stdio;
    use tokio::process::Command;

    tracing::info!("Attempting recovery: force uninstall Helm release '{}' in '{}'", release_name, namespace);

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
        tracing::warn!("Normal uninstall failed, falling back to secret cleanup: {}", String::from_utf8_lossy(&output.stderr));
        let kubectl = if helm_binary.ends_with(".exe") {
            "kubectl.exe"
        } else {
            "kubectl"
        };
        return clean_helm_release(kubectl, namespace).await;
    }

    tracing::info!("Recovery succeeded: force uninstall Helm release '{}' in '{}'", release_name, namespace);
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

    tracing::info!("Attempting recovery: recreate k3d cluster with memory limit '{}'", memory_limit);

    // Force delete existing cluster
    match Command::new("k3d")
        .args(["cluster", "delete", "claude-code"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
    {
        Ok(output) if !output.status.success() => {
            tracing::warn!("k3d cluster delete returned non-zero: {}", String::from_utf8_lossy(&output.stderr));
        }
        Err(e) => {
            tracing::warn!("k3d cluster delete failed: {}", e);
        }
        _ => {
            tracing::debug!("k3d cluster delete succeeded");
        }
    }

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

    let result = CmdResult {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into(),
        stderr: String::from_utf8_lossy(&output.stderr).into(),
    };
    if result.success {
        tracing::info!("Recovery succeeded: recreate k3d cluster");
    } else {
        tracing::warn!("Recovery failed: recreate k3d cluster: {}", result.stderr);
    }
    Ok(result)
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

// =============================================================================
// Log failure analysis (pure functions, no UI dependencies)
// =============================================================================

/// Known failure patterns to scan for in log output.
pub const FAILURE_PATTERNS: &[&str] = &[
    "Warning", "Unhealthy", "OOMKilled",
    "CrashLoopBackOff", "Error", "Failed",
];

/// Scan log text for known failure patterns and return a summary prefix if any are found.
/// Returns `Some("warning Detected issues: X, Y\n---\n")` or `None` if the log is clean.
pub fn detect_failure_patterns(log_text: &str) -> Option<String> {
    let detected: Vec<&str> = FAILURE_PATTERNS
        .iter()
        .filter(|pat| log_text.contains(**pat))
        .copied()
        .collect();
    if detected.is_empty() {
        None
    } else {
        Some(format!(
            "\u{26A0} Detected issues: {}\n---\n",
            detected.join(", ")
        ))
    }
}

/// POD-32: Annotate log lines containing failure patterns with a visual marker prefix.
/// Lines matching known failure patterns get prefixed with "[!] " for visual distinction.
pub fn highlight_failure_lines(log_text: &str) -> String {
    log_text
        .lines()
        .map(|line| {
            let has_failure = FAILURE_PATTERNS.iter().any(|pat| line.contains(pat));
            if has_failure {
                format!("[!] {}", line)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// PRJ-53: Suggest remediation hints for common Docker build failures.
pub fn build_remediation_hint(error_text: &str) -> Option<&'static str> {
    let lower = error_text.to_lowercase();
    if lower.contains("no space left on device") || lower.contains("disk space") {
        Some("Hint: Free disk space or run image cleanup (Settings > Image Retention)")
    } else if lower.contains("connection refused") || lower.contains("dial tcp") || lower.contains("timeout") {
        Some("Hint: Check network connectivity and Docker daemon status")
    } else if lower.contains("not found") && lower.contains("dockerfile") {
        Some("Hint: Ensure a Dockerfile exists in the project root or .claude/ directory")
    } else if lower.contains("permission denied") {
        Some("Hint: Check file permissions on the project directory")
    } else if lower.contains("pull access denied") || lower.contains("authentication required") {
        Some("Hint: Check Docker registry authentication (docker login)")
    } else if lower.contains("exec format error") {
        Some("Hint: The base image architecture may not match your system (try a multi-arch image)")
    } else {
        None
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

    #[test]
    fn recovery_action_manual_steps() {
        // Every variant must provide non-empty manual fix instructions
        let actions = vec![
            RecoveryAction::FixNamespaceOwnership,
            RecoveryAction::CleanHelmRelease,
            RecoveryAction::ForceReinstallHelm,
            RecoveryAction::RecreateK3dCluster,
            RecoveryAction::DeletePod("test-pod".into()),
            RecoveryAction::ReimportImage { image: "img".into(), pod: "p".into() },
        ];
        for action in &actions {
            let steps = action.manual_steps();
            assert!(!steps.is_empty(), "manual_steps() must not be empty for {:?}", action);
            assert!(steps.starts_with("Manual fix:"), "manual_steps() should start with 'Manual fix:' for {:?}", action);
        }
    }

    #[test]
    fn manual_steps_contain_relevant_commands() {
        assert!(RecoveryAction::FixNamespaceOwnership.manual_steps().contains("kubectl"));
        assert!(RecoveryAction::CleanHelmRelease.manual_steps().contains("helm"));
        assert!(RecoveryAction::ForceReinstallHelm.manual_steps().contains("helm uninstall"));
        assert!(RecoveryAction::RecreateK3dCluster.manual_steps().contains("k3d cluster"));
        assert!(RecoveryAction::DeletePod("x".into()).manual_steps().contains("kubectl delete pod"));
        assert!(RecoveryAction::ReimportImage { image: "i".into(), pod: "p".into() }
            .manual_steps().contains("k3d image import"));
    }

    // =========================================================================
    // CLU-33: Terraform state corruption detection
    // =========================================================================

    #[test]
    fn terraform_state_corrupt_backend_changed() {
        assert!(is_terraform_state_corrupt("Error: Backend configuration changed"));
    }

    #[test]
    fn terraform_state_corrupt_version_mismatch() {
        assert!(is_terraform_state_corrupt("state snapshot was created by terraform v1.5.0"));
    }

    #[test]
    fn terraform_state_corrupt_plugins_missing() {
        assert!(is_terraform_state_corrupt("Error: Required plugins are not installed"));
    }

    #[test]
    fn terraform_state_corrupt_lock_file() {
        assert!(is_terraform_state_corrupt("dependency lock file does not match"));
    }

    #[test]
    fn terraform_state_corrupt_loading_error() {
        assert!(is_terraform_state_corrupt("Error loading state: the state file is corrupt"));
    }

    #[test]
    fn terraform_state_not_corrupt_normal_error() {
        assert!(!is_terraform_state_corrupt("Error: No configuration files"));
        assert!(!is_terraform_state_corrupt("Error creating k3d cluster: already exists"));
        assert!(!is_terraform_state_corrupt(""));
    }

    // =========================================================================
    // Edge case tests for diagnosis functions
    // =========================================================================

    #[test]
    fn helm_empty_stderr() {
        assert_eq!(diagnose_helm_failure(""), None);
    }

    #[test]
    fn helm_case_insensitive_another_operation() {
        let stderr = "Error: UPGRADE FAILED: ANOTHER OPERATION (INSTALL/UPGRADE/ROLLBACK) IS IN PROGRESS";
        assert_eq!(diagnose_helm_failure(stderr), Some(RecoveryAction::CleanHelmRelease));
    }

    #[test]
    fn helm_multiline_error_with_namespace() {
        let stderr = "Error: UPGRADE FAILED: post-install resources already exist\n\
                       namespaces \"claude-code\" already exists\n\
                       unable to continue with install";
        assert_eq!(diagnose_helm_failure(stderr), Some(RecoveryAction::FixNamespaceOwnership));
    }

    #[test]
    fn cluster_empty_stderr() {
        assert_eq!(diagnose_cluster_failure(""), None);
    }

    #[test]
    fn cluster_k3d_cluster_not_found() {
        let stderr = "FATA[0000] Failed to get cluster: k3d cluster 'claude-code' does not exist";
        assert_eq!(diagnose_cluster_failure(stderr), Some(RecoveryAction::RecreateK3dCluster));
    }

    #[test]
    fn cluster_certificate_not_yet_valid() {
        let stderr = "Unable to connect to the server: x509: certificate is not yet valid";
        assert_eq!(diagnose_cluster_failure(stderr), Some(RecoveryAction::RecreateK3dCluster));
    }

    // =========================================================================
    // Pod diagnosis edge cases
    // =========================================================================

    #[test]
    fn pods_crash_loop_backoff_phase() {
        let pods = vec![crate::kubectl::PodStatus {
            name: "pod-1".into(),
            project: "proj".into(),
            phase: "CrashLoopBackOff".into(),
            ready: false,
            restart_count: 10,
            age: "30m".into(),
            warnings: vec![],
            exposed: false,
            container_port: 0,
            selected: false,
        }];
        let actions = diagnose_pod_issues(&pods);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], RecoveryAction::DeletePod("pod-1".into()));
    }

    #[test]
    fn pods_restart_count_exactly_5_no_action() {
        let pods = vec![crate::kubectl::PodStatus {
            name: "pod-1".into(),
            project: "proj".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 5,
            age: "1h".into(),
            warnings: vec![],
            exposed: false,
            container_port: 0,
            selected: false,
        }];
        let actions = diagnose_pod_issues(&pods);
        assert!(actions.is_empty(), "restart_count == 5 should NOT trigger delete (> 5 required)");
    }

    #[test]
    fn pods_pending_phase_not_deleted() {
        // Pending pods with high restarts should NOT be deleted (only Running or CrashLoopBackOff)
        let pods = vec![crate::kubectl::PodStatus {
            name: "pod-1".into(),
            project: "proj".into(),
            phase: "Pending".into(),
            ready: false,
            restart_count: 10,
            age: "1h".into(),
            warnings: vec![],
            exposed: false,
            container_port: 0,
            selected: false,
        }];
        let actions = diagnose_pod_issues(&pods);
        assert!(actions.is_empty(), "Pending pods should not be deleted");
    }

    #[test]
    fn pods_empty_list() {
        let actions = diagnose_pod_issues(&[]);
        assert!(actions.is_empty());
        let image_actions = diagnose_image_issues(&[]);
        assert!(image_actions.is_empty());
    }

    #[test]
    fn image_err_image_never_pull() {
        let pods = vec![crate::kubectl::PodStatus {
            name: "pod-1".into(),
            project: "myapp".into(),
            phase: "ErrImageNeverPull".into(),
            ready: false,
            restart_count: 0,
            age: "2m".into(),
            warnings: vec![],
            exposed: false,
            container_port: 0,
            selected: false,
        }];
        let actions = diagnose_image_issues(&pods);
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0],
            RecoveryAction::ReimportImage {
                image: "claude-code-myapp:latest".into(),
                pod: "pod-1".into(),
            }
        );
    }

    #[test]
    fn image_pending_pod_triggers_reimport() {
        let pods = vec![crate::kubectl::PodStatus {
            name: "pod-1".into(),
            project: "proj".into(),
            phase: "Pending".into(),
            ready: false,
            restart_count: 0,
            age: "5m".into(),
            warnings: vec![],
            exposed: false,
            container_port: 0,
            selected: false,
        }];
        let actions = diagnose_image_issues(&pods);
        assert_eq!(actions.len(), 1, "Pending pods should trigger image reimport");
    }

    #[test]
    fn image_running_pod_not_reimported() {
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

    #[test]
    fn multiple_pods_multiple_issues() {
        let pods = vec![
            crate::kubectl::PodStatus {
                name: "crash-pod".into(),
                project: "proj-a".into(),
                phase: "CrashLoopBackOff".into(),
                ready: false,
                restart_count: 15,
                age: "2h".into(),
                warnings: vec![],
                exposed: false,
                container_port: 0,
                selected: false,
            },
            crate::kubectl::PodStatus {
                name: "ok-pod".into(),
                project: "proj-b".into(),
                phase: "Running".into(),
                ready: true,
                restart_count: 0,
                age: "1h".into(),
                warnings: vec![],
                exposed: false,
                container_port: 0,
                selected: false,
            },
            crate::kubectl::PodStatus {
                name: "another-crash".into(),
                project: "proj-c".into(),
                phase: "Running".into(),
                ready: false,
                restart_count: 8,
                age: "30m".into(),
                warnings: vec![],
                exposed: false,
                container_port: 0,
                selected: false,
            },
        ];
        let actions = diagnose_pod_issues(&pods);
        assert_eq!(actions.len(), 2, "two pods with restart_count > 5 should be flagged");
    }

    // =========================================================================
    // Recovery tracker edge cases
    // =========================================================================

    #[test]
    fn recovery_tracker_cluster_limits() {
        let mut tracker = RecoveryTracker::new();
        assert!(tracker.can_retry_cluster());
        tracker.record_cluster_attempt();
        assert!(tracker.can_retry_cluster());
        tracker.record_cluster_attempt();
        assert!(!tracker.can_retry_cluster());
    }

    #[test]
    fn recovery_tracker_helm_and_cluster_independent() {
        let mut tracker = RecoveryTracker::new();
        tracker.record_helm_attempt();
        tracker.record_helm_attempt();
        assert!(!tracker.can_retry_helm());
        assert!(tracker.can_retry_cluster(), "cluster should be independent from helm");
    }

    #[test]
    fn recovery_tracker_reset_clears_both() {
        let mut tracker = RecoveryTracker::new();
        tracker.record_helm_attempt();
        tracker.record_helm_attempt();
        tracker.record_cluster_attempt();
        tracker.record_cluster_attempt();
        assert!(!tracker.can_retry_helm());
        assert!(!tracker.can_retry_cluster());
        tracker.reset();
        assert!(tracker.can_retry_helm());
        assert!(tracker.can_retry_cluster());
    }

    #[test]
    fn reimport_image_description() {
        let action = RecoveryAction::ReimportImage {
            image: "claude-code-myapp:latest".into(),
            pod: "pod-abc".into(),
        };
        let desc = action.description();
        assert!(desc.contains("claude-code-myapp:latest"));
        assert!(desc.contains("pod-abc"));
    }

    // =========================================================================
    // detect_failure_patterns (POD-19)
    // =========================================================================

    #[test]
    fn detect_failure_patterns_oomkilled() {
        let log = "Container was OOMKilled at 12:34:00";
        let result = detect_failure_patterns(log);
        assert!(result.is_some());
        let summary = result.unwrap();
        assert!(summary.contains("OOMKilled"), "should detect OOMKilled, got: {}", summary);
    }

    #[test]
    fn detect_failure_patterns_crashloopbackoff() {
        let log = "Back-off restarting failed container\nCrashLoopBackOff";
        let result = detect_failure_patterns(log);
        assert!(result.is_some());
        let summary = result.unwrap();
        assert!(summary.contains("CrashLoopBackOff"), "should detect CrashLoopBackOff, got: {}", summary);
    }

    #[test]
    fn detect_failure_patterns_multiple() {
        let log = "Warning: OOMKilled container restarted\nCrashLoopBackOff detected\nFailed to pull image";
        let result = detect_failure_patterns(log);
        assert!(result.is_some());
        let summary = result.unwrap();
        assert!(summary.contains("Warning"), "should detect Warning");
        assert!(summary.contains("OOMKilled"), "should detect OOMKilled");
        assert!(summary.contains("CrashLoopBackOff"), "should detect CrashLoopBackOff");
        assert!(summary.contains("Failed"), "should detect Failed");
    }

    #[test]
    fn detect_failure_patterns_clean_log() {
        let log = "Server started on port 8080\nListening for connections\nRequest handled successfully";
        let result = detect_failure_patterns(log);
        assert!(result.is_none(), "clean log should not trigger any pattern");
    }

    #[test]
    fn detect_failure_patterns_case_sensitive() {
        // "oomkilled" (lowercase) should NOT match "OOMKilled"
        let log = "container was oomkilled and crashloopbackoff";
        let result = detect_failure_patterns(log);
        assert!(result.is_none(), "patterns are case-sensitive, lowercase should not match");
    }

    #[test]
    fn detect_failure_patterns_error_in_stack_trace() {
        // "Error" appears in a normal stack trace context - it still matches since
        // the scanner is a simple substring match
        let log = "at Object.Error (native)\n  at parseJSON (/app/index.js:42:11)";
        let result = detect_failure_patterns(log);
        assert!(result.is_some(), "Error in stack trace should still be detected");
        let summary = result.unwrap();
        assert!(summary.contains("Error"), "should contain Error");
        // Should NOT contain patterns that aren't present
        assert!(!summary.contains("OOMKilled"), "should not contain OOMKilled");
        assert!(!summary.contains("CrashLoopBackOff"), "should not contain CrashLoopBackOff");
    }

    #[test]
    fn detect_failure_patterns_summary_format() {
        let log = "Warning: something went wrong";
        let result = detect_failure_patterns(log).unwrap();
        assert!(result.starts_with("\u{26A0} Detected issues:"), "should start with warning emoji prefix");
        assert!(result.ends_with("\n---\n"), "should end with separator");
    }

    #[test]
    fn detect_failure_patterns_empty_log() {
        let result = detect_failure_patterns("");
        assert!(result.is_none());
    }

    #[test]
    fn detect_failure_patterns_unhealthy() {
        let log = "Readiness probe failed: Unhealthy";
        let result = detect_failure_patterns(log);
        assert!(result.is_some());
        let summary = result.unwrap();
        assert!(summary.contains("Unhealthy"));
    }

    #[test]
    fn detect_failure_patterns_all_patterns_present() {
        let log = "Warning Unhealthy OOMKilled CrashLoopBackOff Error Failed";
        let result = detect_failure_patterns(log).unwrap();
        // All six patterns should be listed
        for pat in FAILURE_PATTERNS {
            assert!(result.contains(pat), "should contain '{}' in: {}", pat, result);
        }
    }

    // POD-32: highlight_failure_lines tests
    #[test]
    fn highlight_failure_lines_marks_error_lines() {
        let log = "Starting app\nError: connection refused\nListening on 8080";
        let highlighted = highlight_failure_lines(log);
        let lines: Vec<&str> = highlighted.lines().collect();
        assert_eq!(lines[0], "Starting app");
        assert_eq!(lines[1], "[!] Error: connection refused");
        assert_eq!(lines[2], "Listening on 8080");
    }

    #[test]
    fn highlight_failure_lines_no_failures() {
        let log = "Starting app\nListening on 8080\nReady";
        let highlighted = highlight_failure_lines(log);
        assert!(!highlighted.contains("[!]"));
        assert_eq!(highlighted, log);
    }

    #[test]
    fn highlight_failure_lines_multiple_patterns() {
        let log = "CrashLoopBackOff detected\nWarning: OOMKilled";
        let highlighted = highlight_failure_lines(log);
        assert!(highlighted.lines().all(|l| l.starts_with("[!]")));
    }

    // PRJ-53: build_remediation_hint tests
    #[test]
    fn build_hint_disk_space() {
        let hint = build_remediation_hint("COPY failed: no space left on device");
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("disk space"));
    }

    #[test]
    fn build_hint_network() {
        let hint = build_remediation_hint("dial tcp: connection refused");
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("network"));
    }

    #[test]
    fn build_hint_permission() {
        let hint = build_remediation_hint("permission denied while trying to connect");
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("permissions"));
    }

    #[test]
    fn build_hint_none_for_unknown() {
        let hint = build_remediation_hint("some random error that we don't know about");
        assert!(hint.is_none());
    }

    #[test]
    fn build_hint_auth() {
        let hint = build_remediation_hint("pull access denied for private-image");
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("registry"));
    }
}
