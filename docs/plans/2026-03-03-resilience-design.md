# Resilience: Memory Limiting, Health Monitoring & Self-Recovery

**Date:** 2026-03-03
**Status:** Approved

## Problem

1. k3d cluster consumed all host RAM over 12 hours (no memory cap on Docker container)
2. No per-component health visibility in the UI
3. Known failures require manual intervention (Helm namespace ownership errors, corrupted k3d)

## Design

### 1. Memory Limiting (K3d Cluster Level)

**Goal:** Cap k3d Docker container memory at a configurable percentage of system RAM (default 80%).

**Config:**
- New field `cluster_memory_percent: u8` in `AppConfig` (default: 80, range: 50-95)
- New Terraform variable `cluster_memory_limit` (string, e.g. `"12800m"`)

**System RAM detection:**
- Add `sysinfo` crate to detect total physical memory
- In `AppState::write_terraform_vars()`, compute `total_ram * percent / 100` and write to `terraform.auto.tfvars`

**Terraform:**
- `terraform/variables.tf` — add `cluster_memory_limit` variable
- `terraform/k3s.tf` — modify k3d create: `k3d cluster create claude-code --wait --timeout 120s --servers-memory ${var.cluster_memory_limit}`
- Linux/macOS k3s: not applicable (native process, memory managed via pod limits)

**UI:**
- Settings panel: add "Cluster Memory %" field showing computed value (e.g. `"80% (12.8 GB of 16 GB)"`)
- Configurable range: 50-95%

### 2. Health Monitoring (All Layers)

**Goal:** Background health polling with per-component status visible in the UI.

**New module `src/health.rs`:**

```rust
enum ComponentHealth { Healthy, Degraded, Unhealthy, Unknown }

struct HealthReport {
    docker: ComponentHealth,         // docker info
    cluster: ComponentHealth,        // kubectl cluster-info
    node: ComponentHealth,           // kubectl get nodes -> Ready
    helm_release: ComponentHealth,   // helm status claude-code
    pods: ComponentHealth,           // all pods Running, no CrashLoopBackOff
    memory_usage_percent: Option<f32>,
}
```

**Background poll loop:**
- `tokio::spawn` task runs every 20 seconds
- Checks each component via existing runners (docker, kubectl, helm)
- Updates `AppState` with `HealthReport`
- Pushes to UI via `slint::invoke_from_event_loop`

**Health check commands:**
- Docker: `docker info` (success/fail)
- Cluster: `kubectl cluster-info` (success/fail)
- Node: `kubectl get nodes -o json` (parse Ready condition)
- Helm: `helm status claude-code -n claude-code` (success/fail)
- Pods: `kubectl get pods -n claude-code -o json` (check phases, restart counts)
- Memory: `docker stats` on k3d container (Windows) or `kubectl top node` (Linux)

**UI changes:**
- Extend "Stack Status" card: add Node and Helm Release rows with StatusBadge
- Add memory usage indicator: `"Memory: 6.2 GB / 12.8 GB (48%)"`
- Sidebar status dot derives from worst component (Unhealthy=red, Degraded=yellow, Healthy=green)

**New `app-window.slint` properties:**
- `node-status: string`
- `helm-release-status: string`
- `memory-usage-text: string`

### 3. Self-Recovery (Automatic with Notification)

**Goal:** Detect known failure patterns, fix automatically, log notification to UI.

**New module `src/recovery.rs`:**

Pattern-matching recovery engine with known failure handlers:

| stderr pattern | Fix | Log message |
|---|---|---|
| `invalid ownership metadata` + `label validation error` | Delete namespace labels, re-apply with Helm ownership annotations, retry Helm install | `[Recovery] Fixed namespace ownership labels. Retrying deploy...` |
| k3d + `corrupted` / `connection refused` / `cluster not found` | `k3d cluster delete --force` + recreate with memory limit | `[Recovery] Recreated k3d cluster. Retrying...` |
| Pod `CrashLoopBackOff` with restarts > 5 | Delete pod (Deployment recreates) | `[Recovery] Deleted crash-looping pod {name}.` |
| `ImagePullBackOff` / `ErrImageNeverPull` | Re-import image to k3d, delete pod | `[Recovery] Re-imported image {tag}. Restarting pod.` |

**Namespace ownership fix (detailed):**
1. `kubectl label namespace claude-code app.kubernetes.io/managed-by=Helm --overwrite`
2. `kubectl annotate namespace claude-code meta.helm.sh/release-name=claude-code --overwrite`
3. `kubectl annotate namespace claude-code meta.helm.sh/release-namespace=claude-code --overwrite`
4. Retry `helm upgrade --install`

**Integration points:**
1. **Helm deploy path** — after `install_or_upgrade()` fails, pass stderr to `recovery::try_recover()`. If fix found, execute and retry once.
2. **Health poll loop** — when detecting CrashLoopBackOff/ImagePullBackOff pods, trigger pod-level recovery.
3. **Cluster creation** — if terraform apply fails with k3d errors, attempt cluster recreate.

**Safety guardrails:**
- Max 2 recovery attempts per operation per session (resets on success)
- All recovery logged with `[Recovery]` prefix
- Failed recovery falls through to normal error display
- k3d recreate triggers terraform re-apply (state is stale)

## Files Changed

| File | Change |
|---|---|
| `Cargo.toml` | Add `sysinfo` crate |
| `src/config.rs` | Add `cluster_memory_percent` field |
| `src/app.rs` | Write memory limit to tfvars, add `HealthReport` to state |
| `src/health.rs` | New: health checking logic and background poll |
| `src/recovery.rs` | New: failure pattern matching and auto-fix |
| `src/main.rs` | Start health poll loop, integrate recovery into deploy/helm paths |
| `src/helm.rs` | No changes (recovery wraps existing calls) |
| `src/kubectl.rs` | Add `get_nodes()` method for node readiness check |
| `src/docker.rs` | Add `docker_info()` method for Docker health check |
| `terraform/variables.tf` | Add `cluster_memory_limit` variable |
| `terraform/k3s.tf` | Add `--servers-memory` to k3d create |
| `ui/app-window.slint` | Add node-status, helm-release-status, memory-usage-text properties |
| `ui/components/cluster-panel.slint` | Add Node/Helm rows, memory indicator |
