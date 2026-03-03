# Resilience Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add memory limiting for k3d clusters, background health monitoring for all stack layers, and automatic self-recovery for known failure patterns.

**Architecture:** Three new modules (`health.rs`, `recovery.rs`, and sysinfo integration) layered on top of existing runners. A background tokio task polls health every 20s and triggers recovery when needed. Memory limits flow from config -> terraform.auto.tfvars -> k3d cluster create.

**Tech Stack:** Rust, Slint UI, sysinfo crate, existing kubectl/helm/docker/terraform runners

---

### Task 1: Add sysinfo dependency and cluster_memory_percent to config

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/config.rs`

**Step 1: Add sysinfo to Cargo.toml**

In `Cargo.toml`, add to `[dependencies]`:

```toml
sysinfo = "0.33"
```

**Step 2: Add cluster_memory_percent field to AppConfig**

In `src/config.rs`, add the field to `AppConfig` struct after `memory_limit`:

```rust
pub cluster_memory_percent: u8,
```

In `Default for AppConfig`, add:

```rust
cluster_memory_percent: 80,
```

**Step 3: Write a test for the new config field**

In `src/config.rs` tests, add:

```rust
#[test]
fn default_cluster_memory_percent() {
    let cfg = AppConfig::default();
    assert_eq!(cfg.cluster_memory_percent, 80);
}

#[test]
fn cluster_memory_percent_roundtrip() {
    let tmp = TempDir::new().expect("create temp dir");
    let path = tmp.path().join("config.toml");

    let cfg = AppConfig {
        cluster_memory_percent: 70,
        ..AppConfig::default()
    };
    cfg.save_to(&path).expect("save");
    let loaded = AppConfig::load_from(&path).expect("load");
    assert_eq!(loaded.cluster_memory_percent, 70);
}
```

**Step 4: Update existing tests that construct AppConfig explicitly**

In `src/config.rs` test `serialize_deserialize_roundtrip`, add the field:

```rust
cluster_memory_percent: 90,
```

And assert:

```rust
assert_eq!(loaded.cluster_memory_percent, 90);
```

In `deserialize_missing_optional_fields`, add to the TOML string:

```
cluster_memory_percent = 80
```

In `save_and_load_roundtrip`, add the field and assert it.

**Step 5: Run tests**

Run: `cargo test --lib config`
Expected: All pass

**Step 6: Commit**

```bash
git add Cargo.toml src/config.rs
git commit -m "feat: add cluster_memory_percent config field and sysinfo dep"
```

---

### Task 2: Write memory limit to terraform.auto.tfvars

**Files:**
- Modify: `src/app.rs`

**Step 1: Write a test for memory limit in tfvars**

In `src/app.rs` tests, add:

```rust
#[test]
fn write_terraform_vars_includes_memory_limit() {
    let tmp = TempDir::new().expect("create temp dir");
    let tf_dir = tmp.path().join("terraform");
    std::fs::create_dir(&tf_dir).unwrap();

    let mut state = make_state();
    state.config.terraform_dir = "terraform".to_string();
    state.project_root = tmp.path().to_path_buf();
    state.platform = Platform::Windows;
    state.config.cluster_memory_percent = 75;

    state.write_terraform_vars().expect("write vars");
    let content = std::fs::read_to_string(tf_dir.join("terraform.auto.tfvars")).unwrap();
    assert!(content.contains("platform = \"windows\""));
    assert!(content.contains("cluster_memory_limit = \""));
    // The value should be a number followed by m (megabytes)
    assert!(content.contains("m\""));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib app::tests::write_terraform_vars_includes_memory_limit`
Expected: FAIL (tfvars doesn't include cluster_memory_limit yet)

**Step 3: Implement memory limit calculation in write_terraform_vars**

In `src/app.rs`, add at the top:

```rust
use sysinfo::System;
```

Modify `write_terraform_vars` to also write the memory limit:

```rust
pub fn write_terraform_vars(&self) -> AppResult<()> {
    let platform_str = match self.platform {
        Platform::Linux => "linux",
        Platform::MacOs => "macos",
        Platform::Wsl2 => "wsl2",
        Platform::Windows => "windows",
    };

    let memory_limit = self.compute_cluster_memory_limit();

    let vars_path = PathBuf::from(self.terraform_dir()).join("terraform.auto.tfvars");
    std::fs::write(
        &vars_path,
        format!(
            "platform = \"{}\"\ncluster_memory_limit = \"{}m\"\n",
            platform_str, memory_limit
        ),
    )?;
    Ok(())
}

/// Compute the cluster memory limit in megabytes based on system RAM and config percentage.
fn compute_cluster_memory_limit(&self) -> u64 {
    let mut sys = System::new();
    sys.refresh_memory();
    let total_mb = sys.total_memory() / 1024 / 1024;
    let percent = self.config.cluster_memory_percent.clamp(50, 95) as u64;
    total_mb * percent / 100
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --lib app::tests::write_terraform_vars_includes_memory_limit`
Expected: PASS

**Step 5: Fix the two existing write_terraform_vars tests**

The existing tests `write_terraform_vars_creates_file` and `write_terraform_vars_linux` assert exact string equality. Update them to check for `platform =` using `contains()` instead of exact match, since the file now has two lines.

**Step 6: Run all app tests**

Run: `cargo test --lib app`
Expected: All pass

**Step 7: Commit**

```bash
git add src/app.rs
git commit -m "feat: write cluster memory limit to terraform.auto.tfvars"
```

---

### Task 3: Add terraform variable and update k3d cluster create

**Files:**
- Modify: `terraform/variables.tf`
- Modify: `terraform/k3s.tf`

**Step 1: Add variable to variables.tf**

Add after the `platform` variable:

```hcl
variable "cluster_memory_limit" {
  description = "Memory limit for k3d cluster in megabytes (e.g. 12800m)"
  type        = string
  default     = "12800m"
}
```

**Step 2: Update k3d cluster create command in k3s.tf**

In the `k3d_cluster_create` resource, change the create provisioner command to:

```hcl
provisioner "local-exec" {
    interpreter = ["powershell", "-Command"]
    command     = <<-EOT
      $existing = k3d cluster list -o json | ConvertFrom-Json | Where-Object { $_.name -eq "claude-code" }
      if ($existing) {
        Write-Host "k3d cluster 'claude-code' already exists"
      } else {
        Write-Host "Creating k3d cluster with memory limit ${var.cluster_memory_limit}..."
        k3d cluster create claude-code --wait --timeout 120s --servers-memory ${var.cluster_memory_limit}
        Write-Host "k3d cluster created"
      }
    EOT
  }
```

**Step 3: Commit**

```bash
git add terraform/variables.tf terraform/k3s.tf
git commit -m "feat: add --servers-memory flag to k3d cluster create"
```

---

### Task 4: Add cluster memory percent to UI settings

**Files:**
- Modify: `ui/app-window.slint`
- Modify: `src/main.rs`

**Step 1: Add property to app-window.slint**

In the AppWindow properties section (around line 45), add:

```slint
in-out property <string> cluster-memory-percent: "80";
in property <string> cluster-memory-info: "";
```

In the Settings page "Resource Limits" Card (around line 291), add a new field after the Memory FormField:

```slint
FormField { label: "Cluster Memory %"; text <=> root.cluster-memory-percent; placeholder: "80"; }
Text { text: root.cluster-memory-info; font-size: 11px; color: Theme.text-muted; }
```

**Step 2: Wire the property in main.rs**

In the initial UI state block, add:

```rust
ui.set_cluster_memory_percent(s.config.cluster_memory_percent.to_string().into());
```

Add a helper to compute the info string and set it:

```rust
fn compute_memory_info(percent: u8) -> String {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let total_gb = sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let limit_gb = total_gb * percent as f64 / 100.0;
    format!("{:.1} GB of {:.1} GB", limit_gb, total_gb)
}
```

Set it in the initial state block:

```rust
ui.set_cluster_memory_info(compute_memory_info(s.config.cluster_memory_percent).into());
```

In `on_save_settings`, add:

```rust
s.config.cluster_memory_percent = ui.get_cluster_memory_percent()
    .to_string()
    .parse::<u8>()
    .unwrap_or(80)
    .clamp(50, 95);
```

**Step 3: Run build**

Run: `cargo build`
Expected: Compiles successfully

**Step 4: Commit**

```bash
git add ui/app-window.slint src/main.rs
git commit -m "feat: add cluster memory percent to settings UI"
```

---

### Task 5: Create health.rs module with HealthReport struct

**Files:**
- Create: `src/health.rs`
- Modify: `src/main.rs` (add `mod health;`)

**Step 1: Write tests first**

Create `src/health.rs`:

```rust
use crate::error::AppResult;
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
    /// Return the worst status across all components (for the sidebar indicator).
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

/// Check Docker daemon health.
pub async fn check_docker(docker: &DockerBuilder) -> ComponentHealth {
    if docker.is_running().await {
        ComponentHealth::Healthy
    } else {
        ComponentHealth::Unhealthy
    }
}

/// Check cluster reachability.
pub async fn check_cluster(kubectl: &KubectlRunner) -> ComponentHealth {
    match kubectl.cluster_health().await {
        Ok(true) => ComponentHealth::Healthy,
        Ok(false) => ComponentHealth::Unhealthy,
        Err(_) => ComponentHealth::Unhealthy,
    }
}

/// Check node readiness by parsing `kubectl get nodes -o json`.
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

/// Check Helm release status.
pub async fn check_helm(helm: &HelmRunner) -> ComponentHealth {
    match helm.status().await {
        Ok(r) if r.success => ComponentHealth::Healthy,
        Ok(_) => ComponentHealth::Unhealthy,
        Err(_) => ComponentHealth::Unknown,
    }
}

/// Check pod health. Returns Degraded if any pod is in CrashLoopBackOff or has high restarts.
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

/// Run a full health check across all components.
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
```

**Step 2: Add `mod health;` to main.rs**

In `src/main.rs`, add after `mod helm;`:

```rust
mod health;
```

**Step 3: Add get_nodes() to kubectl.rs**

In `src/kubectl.rs`, add to `KubectlRunner` impl:

```rust
pub async fn get_nodes(&self) -> AppResult<Vec<NodeStatus>> {
    let result = self.run(&["get", "nodes", "-o", "json"]).await?;

    if !result.success {
        return Ok(vec![]);
    }

    let node_list: serde_json::Value = serde_json::from_str(&result.stdout)?;
    let mut nodes = Vec::new();

    if let Some(items) = node_list["items"].as_array() {
        for item in items {
            let name = item["metadata"]["name"]
                .as_str()
                .unwrap_or("unknown")
                .to_string();
            let ready = item["status"]["conditions"]
                .as_array()
                .and_then(|conds| {
                    conds.iter().find(|c| c["type"].as_str() == Some("Ready"))
                })
                .and_then(|c| c["status"].as_str())
                .map(|s| s == "True")
                .unwrap_or(false);
            nodes.push(NodeStatus { name, ready });
        }
    }

    Ok(nodes)
}
```

And add the struct above `KubectlRunner`:

```rust
#[derive(Debug, Clone)]
pub struct NodeStatus {
    pub name: String,
    pub ready: bool,
}
```

**Step 4: Run tests**

Run: `cargo test --lib health`
Expected: All pass

**Step 5: Commit**

```bash
git add src/health.rs src/main.rs src/kubectl.rs
git commit -m "feat: add health module with per-component health checking"
```

---

### Task 6: Add health status properties to UI

**Files:**
- Modify: `ui/app-window.slint`
- Modify: `ui/components/cluster-panel.slint`

**Step 1: Add new properties to app-window.slint**

In the AppWindow properties (around line 27), add:

```slint
in property <string> node-status: "Unknown";
in property <string> helm-release-status: "Unknown";
in property <string> memory-usage-text: "";
```

Pass them to ClusterPanel:

```slint
node-status: root.node-status;
helm-release-status: root.helm-release-status;
memory-usage-text: root.memory-usage-text;
```

**Step 2: Update cluster-panel.slint**

Add new input properties:

```slint
in property <string> node-status: "Unknown";
in property <string> helm-release-status: "Unknown";
in property <string> memory-usage-text: "";
```

Add new rows inside the "Stack Status" Card's VerticalLayout, after the existing Containers row:

```slint
HorizontalLayout {
    spacing: 8px;
    Text { text: "Node"; font-size: 13px; color: Theme.text; horizontal-stretch: 1; }
    StatusBadge { status: node-status; width: 100px; }
}

HorizontalLayout {
    spacing: 8px;
    Text { text: "Helm Release"; font-size: 13px; color: Theme.text; horizontal-stretch: 1; }
    StatusBadge { status: helm-release-status; width: 100px; }
}
```

Add memory usage text after the Card:

```slint
if memory-usage-text != "" : Text {
    text: memory-usage-text;
    font-size: 12px;
    color: Theme.text-muted;
}
```

**Step 3: Add "Degraded" to StatusBadge**

In `ui/components/status-badge.slint`, add to the background conditions:

```slint
if status == "Degraded" { return Theme.warning; }
```

(Add it alongside the existing `Pending` / `Starting` line.)

**Step 4: Run build**

Run: `cargo build`
Expected: Compiles

**Step 5: Commit**

```bash
git add ui/app-window.slint ui/components/cluster-panel.slint ui/components/status-badge.slint
git commit -m "feat: add node, helm release, and memory status to cluster panel UI"
```

---

### Task 7: Wire background health poll loop in main.rs

**Files:**
- Modify: `src/main.rs`

**Step 1: Add background health loop**

After all the `ui.on_*` callbacks are registered (before `ui.run()`), add:

```rust
// --- Background health poll ---
{
    let ui_handle = ui.as_weak();
    let state = state.clone();
    let rt_handle = rt.handle().clone();

    rt_handle.spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(20)).await;

            let (docker_builder, kubectl, helm_runner) = {
                let s = state.lock().unwrap();
                (s.docker_builder(), s.kubectl_runner(), s.helm_runner())
            };

            let (report, pods) = health::full_check(
                &docker_builder,
                &kubectl,
                &helm_runner,
            ).await;

            let ui = ui_handle.clone();
            let state2 = state.clone();
            slint::invoke_from_event_loop(move || {
                {
                    let mut s = state2.lock().unwrap();
                    s.cluster_healthy = report.overall() == health::ComponentHealth::Healthy;
                    s.pods = pods;
                }

                if let Some(ui) = ui.upgrade() {
                    ui.set_docker_status(report.docker.as_str().into());
                    ui.set_cluster_status(report.cluster.as_str().into());
                    ui.set_node_status(report.node.as_str().into());
                    ui.set_helm_release_status(report.helm_release.as_str().into());
                    ui.set_containers_status(report.pods.as_str().into());
                    ui.set_memory_usage_text(report.memory_usage_text().into());

                    // Derive sidebar status from overall
                    // (cluster-status already drives the sidebar dot)
                }

                sync_pods(&ui, &state2);
            }).ok();
        }
    });
}
```

**Step 2: Add the `use` import**

At the top of `src/main.rs`, the `mod health;` was already added. No extra use statement needed since we use `health::` qualified paths.

**Step 3: Run build**

Run: `cargo build`
Expected: Compiles

**Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: add background health poll loop every 20 seconds"
```

---

### Task 8: Create recovery.rs module

**Files:**
- Create: `src/recovery.rs`
- Modify: `src/main.rs` (add `mod recovery;`)

**Step 1: Write tests first**

Create `src/recovery.rs`:

```rust
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

/// Fix namespace ownership labels so Helm can manage it.
/// Runs kubectl label/annotate commands.
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
            },
            crate::kubectl::PodStatus {
                name: "pod-2".into(),
                project: "proj".into(),
                phase: "Running".into(),
                ready: true,
                restart_count: 2,
                age: "1h".into(),
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
    fn recovery_action_descriptions() {
        let a = RecoveryAction::FixNamespaceOwnership;
        assert!(a.description().contains("namespace ownership"));

        let b = RecoveryAction::RecreateK3dCluster;
        assert!(b.description().contains("k3d cluster"));

        let c = RecoveryAction::DeletePod("my-pod".into());
        assert!(c.description().contains("my-pod"));
    }
}
```

**Step 2: Add `mod recovery;` to main.rs**

In `src/main.rs`, add after `mod health;`:

```rust
mod recovery;
```

**Step 3: Run tests**

Run: `cargo test --lib recovery`
Expected: All pass

**Step 4: Commit**

```bash
git add src/recovery.rs src/main.rs
git commit -m "feat: add recovery module with failure pattern detection"
```

---

### Task 9: Integrate recovery into Helm deploy path

**Files:**
- Modify: `src/main.rs`
- Modify: `src/app.rs`

**Step 1: Add RecoveryTracker to AppState**

In `src/app.rs`, add to `AppState` struct:

```rust
pub recovery_tracker: recovery::RecoveryTracker,
```

Add to `AppState::new()`:

```rust
recovery_tracker: recovery::RecoveryTracker::new(),
```

Add the import:

```rust
use crate::recovery;
```

Update `make_state()` in tests to include:

```rust
recovery_tracker: crate::recovery::RecoveryTracker::new(),
```

**Step 2: Integrate into Helm deploy in main.rs**

In the `on_launch_selected` callback, after the Helm `install_or_upgrade` call (around line 477), wrap the error handling to attempt recovery:

Replace the direct error handling with a recovery-aware version. The key change is in the Helm deploy section:

```rust
// Deploy via Helm
append_to_tab(&ui, 0, "Deploying via Helm...");

let credentials_path = dirs::home_dir()
    .map(|h| h.join(".claude").to_string_lossy().to_string())
    .unwrap_or_default();

let extra_args: Vec<(&str, &str)> = if !credentials_path.is_empty() {
    vec![("claude.credentialsPath", &credentials_path)]
} else {
    vec![]
};

let mut result = helm_runner
    .install_or_upgrade(&project_tuples, &extra_args)
    .await;

// Attempt recovery on failure
if let Ok(ref r) = result {
    if !r.success {
        if let Some(action) = recovery::diagnose_helm_failure(&r.stderr) {
            let can_retry = {
                let s = state.lock().unwrap();
                s.recovery_tracker.can_retry_helm()
            };
            if can_retry {
                append_to_tab(&ui, 0, &action.description());
                if let RecoveryAction::FixNamespaceOwnership = action {
                    let kubectl_bin = {
                        let s = state.lock().unwrap();
                        platform::kubectl_binary(&s.platform).to_string()
                    };
                    let fix_result = recovery::fix_namespace_ownership(
                        &kubectl_bin,
                        "claude-code",
                    ).await;

                    match fix_result {
                        Ok(fr) if fr.success => {
                            {
                                let mut s = state.lock().unwrap();
                                s.recovery_tracker.record_helm_attempt();
                            }
                            append_to_tab(&ui, 0, "[Recovery] Fixed. Retrying Helm install...");
                            result = helm_runner
                                .install_or_upgrade(&project_tuples, &extra_args)
                                .await;
                        }
                        Ok(fr) => {
                            append_to_tab(&ui, 0, &format!("[Recovery] Fix failed: {}", fr.stderr));
                        }
                        Err(e) => {
                            append_to_tab(&ui, 0, &format!("[Recovery] Fix error: {}", e));
                        }
                    }
                }
            }
        }
    }
}

// Reset tracker on success
if let Ok(ref r) = result {
    if r.success {
        let mut s = state.lock().unwrap();
        s.recovery_tracker.reset();
    }
}
```

Note: you'll need to add `use recovery::RecoveryAction;` at the top of the async block or use the fully qualified path.

**Step 3: Run build**

Run: `cargo build`
Expected: Compiles

**Step 4: Commit**

```bash
git add src/main.rs src/app.rs
git commit -m "feat: integrate recovery into Helm deploy path with retry"
```

---

### Task 10: Integrate pod recovery into health poll loop

**Files:**
- Modify: `src/main.rs`

**Step 1: Add pod recovery to the health poll loop**

In the background health poll loop (Task 7), after the health check, add pod recovery:

```rust
// Check for recoverable pod issues
let pod_actions = recovery::diagnose_pod_issues(&pods);
for action in pod_actions {
    if let recovery::RecoveryAction::DeletePod(ref pod_name) = action {
        let kubectl2 = {
            let s = state.lock().unwrap();
            s.kubectl_runner()
        };
        let _ = kubectl2.delete_pod(pod_name).await;

        let msg = format!("[Recovery] Deleted crash-looping pod {}. Deployment will recreate.", pod_name);
        let state3 = state.clone();
        let ui3 = ui_handle.clone();
        slint::invoke_from_event_loop(move || {
            append_log(&state3, &msg);
            sync_log(&ui3, &state3);
        }).ok();
    }
}
```

Insert this between the `full_check` call and the `slint::invoke_from_event_loop` that updates the UI.

**Step 2: Run build**

Run: `cargo build`
Expected: Compiles

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: integrate pod recovery into background health poll"
```

---

### Task 11: Run full test suite and final verification

**Files:** None (verification only)

**Step 1: Run all tests**

Run: `cargo test`
Expected: All tests pass

**Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

**Step 3: Build release**

Run: `cargo build --release`
Expected: Builds successfully

**Step 4: Final commit if any fixes needed**

If clippy or tests flagged issues, fix and commit.

---

## Summary of all files changed

| File | Action |
|---|---|
| `Cargo.toml` | Add `sysinfo = "0.33"` |
| `src/config.rs` | Add `cluster_memory_percent: u8` field |
| `src/app.rs` | Memory limit in tfvars, `RecoveryTracker` in state, sysinfo import |
| `src/health.rs` | **New:** `HealthReport`, `ComponentHealth`, check functions |
| `src/recovery.rs` | **New:** Pattern matching, `RecoveryAction`, `RecoveryTracker`, fix functions |
| `src/kubectl.rs` | Add `NodeStatus` struct, `get_nodes()` method |
| `src/main.rs` | Add `mod health/recovery`, health poll loop, recovery in deploy, memory UI wiring |
| `terraform/variables.tf` | Add `cluster_memory_limit` variable |
| `terraform/k3s.tf` | Add `--servers-memory` to k3d create |
| `ui/app-window.slint` | Add `node-status`, `helm-release-status`, `memory-usage-text`, `cluster-memory-percent` properties |
| `ui/components/cluster-panel.slint` | Add Node/Helm rows, memory text |
| `ui/components/status-badge.slint` | Add "Degraded" status color |
