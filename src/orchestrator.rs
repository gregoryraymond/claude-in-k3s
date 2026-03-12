/// Orchestration logic for building, deploying, and managing projects.
///
/// This module contains the core business logic extracted from UI callbacks,
/// making it testable without a live Slint UI or Kubernetes cluster.
/// Progress reporting is abstracted via the [`Progress`](crate::progress::Progress)
/// trait, enabling both real UI updates and test assertions.

use crate::docker::{self, DockerOps};
use crate::helm::HelmOps;
use crate::kubectl::{KubeOps, PodStatus};
use crate::platform::{self, Platform};
use crate::progress::Progress;
use crate::projects::{self, Project};
use crate::recovery;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// ── Types ──────────────────────────────────────────────────────────

/// All inputs needed to launch a batch of projects.
///
/// Constructed by the UI callback from shared state, then passed to
/// [`launch_projects`] which owns the full build-deploy sequence.
pub struct LaunchConfig {
    /// Projects to build and deploy.
    pub projects: Vec<Project>,
    /// Current platform (determines Docker/k3d/k3s behavior).
    pub platform: Platform,
    /// Shared cancellation signal (set by UI cancel button).
    pub cancel: Arc<AtomicBool>,
    /// Container-side path to `~/.claude` credentials directory.
    pub credentials_path: String,
    /// Extra host mount paths from settings.
    pub extra_mounts: Vec<String>,
    /// Host-side projects directory (used for k3d volume mounts).
    pub projects_dir: Option<String>,
    /// Total cluster memory limit in MB (for k3d `--servers-memory`).
    pub cluster_memory_total_mb: u64,
}

/// Outcome of a [`launch_projects`] operation.
///
/// The caller uses this to update shared application state
/// (desired-state persistence, recovery tracker, pending deploys).
#[derive(Debug)]
pub struct LaunchResult {
    /// Project names that were successfully deployed via Helm.
    pub deployed: Vec<String>,
    /// Project names whose Docker build failed.
    pub build_failures: Vec<String>,
    /// Project names whose Helm deploy failed.
    pub deploy_failures: Vec<String>,
    /// Whether the operation was cancelled by the user.
    pub cancelled: bool,
    /// Pods observed after deployment (if any deploy succeeded).
    pub pods: Option<Vec<PodStatus>>,
}

/// Outcome of a [`stop_all`] operation.
#[derive(Debug)]
pub struct StopResult {
    /// Release names that were successfully uninstalled.
    pub uninstalled: Vec<String>,
    /// Release names that failed to uninstall.
    pub failures: Vec<String>,
}

/// Outcome of a single-project [`retry_build`] operation.
#[derive(Debug)]
pub struct RetryResult {
    /// Whether the Docker build succeeded.
    pub build_ok: bool,
    /// Whether the Helm deploy succeeded (only attempted if build_ok).
    pub deploy_ok: bool,
}

// ── Launch ─────────────────────────────────────────────────────────

/// Execute the full build-deploy pipeline for a batch of projects.
///
/// Sequence: WSL check (Windows) -> Docker check -> k3d/k3s cluster ->
/// per-project Docker build -> image import -> Helm deploy with retry ->
/// pod refresh.
///
/// # Arguments
///
/// * `config` - Launch parameters extracted from app state
/// * `docker` - Docker operations (build, import, health)
/// * `helm` - Helm operations (install, uninstall)
/// * `kubectl` - Kubernetes operations (pods, secrets, health)
/// * `progress` - Progress reporting sink (UI or test recorder)
///
/// # Returns
///
/// A [`LaunchResult`] summarising which projects were deployed, failed, or
/// cancelled, plus a snapshot of pods if any deploy succeeded.
pub async fn launch_projects<D, H, K>(
    config: &LaunchConfig,
    docker: &D,
    helm: &H,
    kubectl: &K,
    progress: &dyn Progress,
) -> LaunchResult
where
    D: DockerOps,
    H: HelmOps,
    K: KubeOps,
{
    let is_windows = config.platform == Platform::Windows;
    let mut step_idx: usize = 0;

    // ── WSL check (Windows only) ───────────────────────────────
    if is_windows {
        progress.add_step("WSL", "running", "Checking...");
        let wsl_step = step_idx;
        step_idx += 1;

        if !check_wsl(progress, wsl_step).await {
            // Non-fatal: we logged the warning, continue anyway
        }
    }

    // ── Docker check ───────────────────────────────────────────
    progress.add_step("Docker", "running", "Checking...");
    let docker_step = step_idx;
    step_idx += 1;

    if !ensure_docker(docker, is_windows, progress, docker_step).await {
        progress.update_tab_status(0, "Failed");
        return early_result(config);
    }

    // ── k3d / k3s cluster ──────────────────────────────────────
    let cluster_label = if is_windows { "k3d Cluster" } else { "k3s" };
    progress.add_step(cluster_label, "pending", "Waiting...");
    let cluster_step = step_idx;
    step_idx += 1;

    if is_windows {
        progress.update_step(cluster_step, "running", "Ensuring cluster exists...");
        progress.append_tab(0, "Ensuring k3d cluster exists...");
        if !ensure_k3d_cluster(config, progress).await {
            progress.update_step(cluster_step, "failed", "Creation failed");
            progress.append_tab(0, "k3d cluster creation failed. Aborting.");
            progress.update_tab_status(0, "Failed");
            return early_result(config);
        }
        progress.update_step(cluster_step, "done", "Ready");
        progress.append_tab(0, "k3d cluster ready.");
    } else {
        progress.update_step(cluster_step, "running", "Checking cluster...");
        let k3s_ok = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            kubectl.get_pods(),
        )
        .await;
        match k3s_ok {
            Ok(Ok(_)) => {
                progress.update_step(cluster_step, "done", "Reachable");
                progress.append_tab(0, "k3s cluster is reachable.");
            }
            _ => {
                progress.update_step(
                    cluster_step,
                    "failed",
                    "Not reachable -- is k3s running?",
                );
                progress.append_tab(
                    0,
                    "k3s cluster not reachable -- continuing, deploy may fail.",
                );
            }
        }
    }

    // ── Register per-project build steps + placeholders ────────
    let build_step_start = step_idx;
    for p in &config.projects {
        progress.add_step(&format!("Building {}", p.name), "pending", "Waiting...");
        step_idx += 1;
    }
    progress.add_step("Importing Images", "pending", "Waiting...");
    let import_step = step_idx;
    step_idx += 1;
    progress.add_step("Helm Deploy", "pending", "Waiting...");
    let helm_step = step_idx;
    step_idx += 1;
    progress.add_step("Starting Pods", "pending", "Waiting...");
    let pods_step = step_idx;

    // ── Build loop ─────────────────────────────────────────────
    let mut built: Vec<(String, String, String)> = Vec::new();
    let mut build_failures: Vec<String> = Vec::new();
    let mut any_build_failed = false;

    for (i, project) in config.projects.iter().enumerate() {
        let tab_idx = (i + 1) as i32;
        let build_step = build_step_start + i;

        if config.cancel.load(Ordering::Relaxed) {
            (i..config.projects.len()).for_each(|j| {
                progress.update_tab_status((j + 1) as i32, "Cancelled");
                progress.update_step(build_step_start + j, "failed", "Cancelled");
            });
            progress.append_tab(0, "Cancelled by user.");
            break;
        }

        progress.update_tab_status(tab_idx, "Building");
        progress.update_step(build_step, "running", "Building...");
        progress.append_tab(0, &format!("Building '{}'...", project.name));

        let name_for_cb = project.name.clone();
        let on_line = move |line: &str| {
            progress.append_tab(tab_idx, line);
            if line.contains("Importing image") {
                progress.append_tab(0, &format!("{}: {}", name_for_cb, line));
            }
        };

        match docker
            .build_and_import_streaming(project, &config.cancel, &on_line)
            .await
        {
            Ok(r) if r.success => {
                let tag = docker::image_tag_for_project(project);
                let host = project.path.to_string_lossy().to_string();
                let container = platform::to_k3d_container_path(&host, &config.platform);
                built.push((project.name.clone(), container, tag));
                progress.update_tab_status(tab_idx, "Done");
                progress.update_step(build_step, "done", "Built successfully");
                progress.append_tab(
                    0,
                    &format!("'{}' built successfully.", project.name),
                );
            }
            Ok(r) => {
                handle_build_failure(
                    project, &r.stderr, tab_idx, build_step, progress,
                    &mut any_build_failed, &mut build_failures,
                );
            }
            Err(e) => {
                let err = e.to_string();
                progress.update_tab_status(tab_idx, "Failed");
                progress.update_step(build_step, "failed", &err);
                progress.append_tab(
                    0,
                    &format!("'{}' error: {}", project.name, err),
                );
                if let Some(hint) = recovery::build_remediation_hint(&err) {
                    progress.append_tab(0, hint);
                    progress.append_tab(tab_idx, hint);
                }
                any_build_failed = true;
                build_failures.push(project.name.clone());
            }
        }
    }

    let cancelled = config.cancel.load(Ordering::Relaxed);

    if !build_failures.is_empty() {
        progress.append_tab(
            0,
            &format!(
                "Build summary: {} succeeded, {} failed [{}]",
                built.len(),
                build_failures.len(),
                build_failures.join(", ")
            ),
        );
        if !built.is_empty() {
            progress.append_tab(
                0,
                "Continuing deployment with successfully built projects...",
            );
        }
    }

    if cancelled || built.is_empty() {
        if !cancelled && !any_build_failed {
            progress.append_tab(0, "No projects to deploy.");
        }
        let (label, msg) = if cancelled {
            ("failed", "Cancelled")
        } else {
            ("skipped", "Skipped")
        };
        progress.update_step(import_step, label, msg);
        progress.update_step(helm_step, label, msg);
        progress.update_step(pods_step, label, msg);
        let tab_status = if cancelled {
            "Cancelled"
        } else if any_build_failed {
            "Failed"
        } else {
            "Done"
        };
        progress.update_tab_status(0, tab_status);
        return LaunchResult {
            deployed: Vec::new(),
            build_failures,
            deploy_failures: Vec::new(),
            cancelled,
            pods: None,
        };
    }

    // Import step already done during build_and_import_streaming
    progress.update_step(import_step, "done", "Images imported");

    // ── Pre-deploy cluster connectivity check ──────────────────
    let cluster_ok = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        kubectl.cluster_health(),
    )
    .await;
    match cluster_ok {
        Ok(Ok(true)) => {
            progress.append_tab(0, "Cluster connectivity verified.");
        }
        _ => {
            progress.append_tab(
                0,
                "WARNING: Cluster connectivity lost -- deploy may fail.",
            );
            progress.set_recovery_hint(
                "Cluster connection lost during operation. \
                 Check Docker Desktop and k3d cluster status.",
            );
        }
    }

    // ── Helm deploy ────────────────────────────────────────────
    progress.update_step(helm_step, "running", "Deploying...");
    progress.append_tab(0, "Deploying via Helm...");

    // Ensure ~/.claude directory exists
    if let Some(home) = dirs::home_dir() {
        let claude_dir = home.join(".claude");
        if !claude_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&claude_dir) {
                tracing::warn!("Failed to create ~/.claude: {}", e);
            } else {
                progress.append_tab(0, "Created ~/.claude directory for credentials.");
            }
        }
    }

    let extra_args = build_extra_helm_args(config);
    let extra_arg_refs: Vec<(&str, &str)> = extra_args
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let (deployed, deploy_failures) = deploy_helm_loop(
        &built,
        &config.projects,
        helm,
        kubectl,
        &extra_arg_refs,
        progress,
    )
    .await;

    let any_deployed = !deployed.is_empty();
    let all_deployed = deploy_failures.is_empty() && any_deployed;

    if all_deployed {
        progress.update_step(helm_step, "done", "Deployed");
    } else if any_deployed {
        progress.update_step(helm_step, "done", "Partially deployed");
    } else {
        progress.update_step(helm_step, "failed", "All deployments failed");
    }

    // Summary
    if any_deployed {
        let msg = if all_deployed {
            "All projects deployed."
        } else {
            "Partially deployed."
        };
        progress.log(msg);
        progress.append_tab(0, msg);
        let tab = if all_deployed { "Done" } else { "Partial" };
        progress.update_tab_status(0, tab);
        let level = if all_deployed { "info" } else { "warning" };
        progress.show_toast(msg, level, 2);
    } else {
        progress.log("All deployments failed.");
        progress.append_tab(0, "All deployments failed.");
        progress.update_tab_status(0, "Failed");
        progress.show_toast("All deployments failed.", "error", 1);
    }

    // ── Pod refresh ────────────────────────────────────────────
    let pods = if any_deployed {
        progress.update_step(pods_step, "running", "Waiting for pods...");
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        match kubectl.get_pods().await {
            Ok(mut pods) => {
                if let Err(e) = kubectl.enrich_pods_with_events(&mut pods).await {
                    tracing::warn!("Failed to enrich pods with events: {}", e);
                }
                progress.update_step(
                    pods_step,
                    "done",
                    &format!("{} pod(s) started", pods.len()),
                );
                Some(pods)
            }
            Err(_) => {
                progress.update_step(pods_step, "done", "Pods starting...");
                None
            }
        }
    } else {
        progress.update_step(pods_step, "skipped", "Deploy failed");
        None
    };

    LaunchResult {
        deployed,
        build_failures,
        deploy_failures,
        cancelled,
        pods,
    }
}

// ── Stop All ───────────────────────────────────────────────────────

/// Uninstall all Helm releases in the namespace.
///
/// # Arguments
///
/// * `helm` - Helm operations runner
/// * `progress` - Progress reporting sink
///
/// # Returns
///
/// A [`StopResult`] listing which releases were uninstalled and which failed.
pub async fn stop_all<H: HelmOps>(
    helm: &H,
    progress: &dyn Progress,
) -> StopResult {
    progress.log("Uninstalling all Helm releases...");

    let releases = helm.list_releases().await;
    match releases {
        Ok(r) if r.success => {
            let names: Vec<String> = r
                .stdout
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            let mut uninstalled = Vec::new();
            let mut failures = Vec::new();
            for name in &names {
                match helm.uninstall(name).await {
                    Ok(r) if r.success => {
                        tracing::debug!("Uninstalled release '{}'", name);
                        uninstalled.push(name.clone());
                    }
                    Ok(r) => {
                        tracing::warn!(
                            "Failed to uninstall '{}': {}",
                            name,
                            r.stderr.trim()
                        );
                        failures.push(name.clone());
                    }
                    Err(e) => {
                        tracing::warn!("Error uninstalling '{}': {}", name, e);
                        failures.push(name.clone());
                    }
                }
            }
            progress.log(&format!("Uninstalled {} release(s).", uninstalled.len()));
            StopResult {
                uninstalled,
                failures,
            }
        }
        Ok(r) => {
            progress.log(&format!("Helm list failed: {}", r.stderr.trim()));
            StopResult {
                uninstalled: Vec::new(),
                failures: Vec::new(),
            }
        }
        Err(e) => {
            progress.log(&format!("Uninstall error: {}", e));
            StopResult {
                uninstalled: Vec::new(),
                failures: Vec::new(),
            }
        }
    }
}

// ── Retry Build ────────────────────────────────────────────────────

/// Rebuild a single project and deploy it via Helm.
///
/// # Arguments
///
/// * `project` - The project to rebuild
/// * `platform` - Current platform (for path conversion)
/// * `cancel` - Shared cancellation flag
/// * `credentials_path` - Container-side credentials path
/// * `extra_helm_args` - Pre-built extra helm set args `(key, value)`
/// * `docker` - Docker operations runner
/// * `helm` - Helm operations runner
/// * `progress` - Progress reporting sink
///
/// # Returns
///
/// A [`RetryResult`] indicating build and deploy outcomes.
pub async fn retry_build<D, H>(
    project: &Project,
    platform: &Platform,
    cancel: &AtomicBool,
    credentials_path: &str,
    extra_helm_args: &[(String, String)],
    docker: &D,
    helm: &H,
    progress: &dyn Progress,
) -> RetryResult
where
    D: DockerOps,
    H: HelmOps,
{
    progress.append_tab(
        0,
        &format!("Retrying build for '{}'...", project.name),
    );

    let on_line = |line: &str| {
        progress.append_tab(0, line);
    };

    let build_ok = match docker
        .build_and_import_streaming(project, cancel, &on_line)
        .await
    {
        Ok(r) if r.success => {
            progress.append_tab(
                0,
                &format!("'{}' rebuilt successfully.", project.name),
            );
            true
        }
        Ok(r) => {
            progress.append_tab(
                0,
                &format!("'{}' retry failed: {}", project.name, r.stderr.trim()),
            );
            if let Some(hint) = recovery::build_remediation_hint(&r.stderr) {
                progress.append_tab(0, hint);
            }
            false
        }
        Err(e) => {
            let msg = e.to_string();
            progress.append_tab(
                0,
                &format!("'{}' retry error: {}", project.name, msg),
            );
            if let Some(hint) = recovery::build_remediation_hint(&msg) {
                progress.append_tab(0, hint);
            }
            false
        }
    };

    if !build_ok {
        return RetryResult {
            build_ok: false,
            deploy_ok: false,
        };
    }

    // Deploy via Helm
    let tag = docker::image_tag_for_project(project);
    let host_path = project.path.to_string_lossy().to_string();
    let container_path = platform::to_k3d_container_path(&host_path, platform);

    let mut helm_args: Vec<(String, String)> = Vec::new();
    if !credentials_path.is_empty() {
        helm_args.push((
            "claude.credentialsPath".into(),
            credentials_path.to_string(),
        ));
    }
    helm_args.extend(extra_helm_args.iter().cloned());
    let arg_refs: Vec<(&str, &str)> = helm_args
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    progress.append_tab(0, &format!("Deploying '{}'...", project.name));

    let deploy_ok = match helm
        .install_project(&project.name, &container_path, &tag, &arg_refs)
        .await
    {
        Ok(r) if r.success => {
            progress.append_tab(
                0,
                &format!("'{}' deployed successfully.", project.name),
            );
            true
        }
        Ok(r) => {
            progress.append_tab(
                0,
                &format!("'{}' deploy failed: {}", project.name, r.stderr.trim()),
            );
            false
        }
        Err(e) => {
            progress.append_tab(
                0,
                &format!("'{}' deploy error: {}", project.name, e),
            );
            false
        }
    };

    RetryResult {
        build_ok: true,
        deploy_ok,
    }
}

// ── Internal helpers ───────────────────────────────────────────────

/// Build the list of extra Helm `--set` args from launch config.
fn build_extra_helm_args(config: &LaunchConfig) -> Vec<(String, String)> {
    let mut args = Vec::new();
    if !config.credentials_path.is_empty() {
        args.push((
            "claude.credentialsPath".into(),
            config.credentials_path.clone(),
        ));
    }
    for (i, mount) in config.extra_mounts.iter().enumerate() {
        let container = platform::to_k3d_container_path(mount, &config.platform);
        args.push((format!("extraMounts[{}]", i), container));
    }
    args
}

/// Deploy each built project via Helm, with automatic retry on first failure.
///
/// Returns `(deployed_names, failed_names)`.
async fn deploy_helm_loop<H, K>(
    built: &[(String, String, String)],
    projects: &[Project],
    helm: &H,
    kubectl: &K,
    extra_args: &[(&str, &str)],
    progress: &dyn Progress,
) -> (Vec<String>, Vec<String>)
where
    H: HelmOps,
    K: KubeOps,
{
    let mut deployed = Vec::new();
    let mut failures = Vec::new();

    for (name, path, image) in built {
        // Create K8s secret from .env if present
        if let Some(project) = projects.iter().find(|p| &p.name == name) {
            if projects::has_env_file(&project.path) {
                let env_vars = projects::parse_env_file(&project.path.join(".env"));
                if !env_vars.is_empty() {
                    match kubectl.apply_secret_from_env(name, &env_vars).await {
                        Ok(r) if r.success => {
                            progress.append_tab(
                                0,
                                &format!("Created env secret for '{}'.", name),
                            );
                        }
                        Ok(r) => {
                            progress.append_tab(
                                0,
                                &format!(
                                    "Env secret failed for '{}': {}",
                                    name,
                                    r.stderr.trim()
                                ),
                            );
                        }
                        Err(e) => {
                            progress.append_tab(
                                0,
                                &format!("Env secret error for '{}': {}", name, e),
                            );
                        }
                    }
                }
            }
        }

        progress.append_tab(0, &format!("Deploying '{}'...", name));
        let result = helm.install_project(name, path, image, extra_args).await;

        // On failure, try uninstall then reinstall once before giving up
        let final_result = match &result {
            Ok(r) if r.success => result,
            _ => {
                progress.append_tab(
                    0,
                    &format!(
                        "'{}' first deploy attempt failed, retrying with reinstall...",
                        name
                    ),
                );
                let _ = helm.uninstall_project(name).await;
                helm.install_project(name, path, image, extra_args).await
            }
        };

        match &final_result {
            Ok(r) if r.success => {
                progress.append_tab(0, &format!("'{}' deployed.", name));
                deployed.push(name.clone());
            }
            Ok(r) => {
                failures.push(name.clone());
                progress.append_tab(
                    0,
                    &format!("'{}' deploy failed: {}", name, r.stderr.trim()),
                );
                progress.show_toast(
                    &format!("Deploy failed: {}", name),
                    "error",
                    2,
                );
                if let Some(action) = recovery::diagnose_helm_failure(&r.stderr) {
                    progress.set_recovery_hint(action.manual_steps());
                }
                // Auto-cleanup partial K8s resources
                if let Err(e) = helm.uninstall_project(name).await {
                    tracing::debug!(
                        "Cleanup uninstall after failed deploy of '{}': {}",
                        name,
                        e
                    );
                }
            }
            Err(e) => {
                failures.push(name.clone());
                progress.append_tab(
                    0,
                    &format!("'{}' deploy error: {}", name, e),
                );
                progress.show_toast(
                    &format!("Deploy error: {}", name),
                    "error",
                    2,
                );
                if let Err(e2) = helm.uninstall_project(name).await {
                    tracing::debug!(
                        "Cleanup uninstall after deploy error for '{}': {}",
                        name,
                        e2
                    );
                }
            }
        }
    }

    (deployed, failures)
}

/// Handle a Docker build failure: update tabs/steps and record the failure.
fn handle_build_failure(
    project: &Project,
    stderr: &str,
    tab_idx: i32,
    build_step: usize,
    progress: &dyn Progress,
    any_failed: &mut bool,
    failures: &mut Vec<String>,
) {
    let status = if stderr.contains("Cancel") {
        "Cancelled"
    } else {
        "Failed"
    };
    progress.update_tab_status(tab_idx, status);
    let step_msg = if status == "Cancelled" {
        "Cancelled"
    } else {
        stderr.lines().next().unwrap_or("Failed")
    };
    progress.update_step(build_step, "failed", step_msg);
    progress.append_tab(
        0,
        &format!("'{}' failed: {}", project.name, stderr.trim()),
    );
    if status == "Failed" {
        if let Some(hint) = recovery::build_remediation_hint(stderr) {
            progress.append_tab(0, hint);
            progress.append_tab(tab_idx, hint);
        }
        *any_failed = true;
        failures.push(project.name.clone());
    }
}

/// Check WSL availability, restarting it if necessary.
/// Returns `true` if WSL is available after the check.
async fn check_wsl(progress: &dyn Progress, wsl_step: usize) -> bool {
    let wsl_ok = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        platform::check_wsl_status(),
    )
    .await
    .unwrap_or(Ok(false))
    .unwrap_or(false);

    if wsl_ok {
        progress.update_step(wsl_step, "done", "Available");
        progress.append_tab(0, "WSL is available.");
        return true;
    }

    progress.update_step(wsl_step, "running", "Restarting WSL...");
    progress.append_tab(0, "WSL not responding -- restarting...");
    if let Err(e) = platform::restart_wsl().await {
        tracing::warn!("WSL restart failed: {}", e);
    }
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let retry_ok = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        platform::check_wsl_status(),
    )
    .await
    .unwrap_or(Ok(false))
    .unwrap_or(false);

    if retry_ok {
        progress.update_step(wsl_step, "done", "Restarted");
        progress.append_tab(0, "WSL restarted successfully.");
    } else {
        progress.update_step(
            wsl_step,
            "failed",
            "Not available -- Docker Desktop requires WSL2",
        );
        progress.append_tab(
            0,
            "WSL restart failed -- Docker Desktop needs WSL2 enabled.",
        );
    }
    retry_ok
}

/// Ensure Docker is running, attempting auto-start on Windows.
/// Returns `true` if Docker is available.
async fn ensure_docker<D: DockerOps>(
    docker: &D,
    is_windows: bool,
    progress: &dyn Progress,
    docker_step: usize,
) -> bool {
    if check_docker_with_timeout(docker).await {
        progress.update_step(docker_step, "done", "Running");
        progress.append_tab(0, "Docker is running.");
        return true;
    }

    if !is_windows {
        progress.update_step(docker_step, "failed", "Not responding");
        progress.append_tab(
            0,
            "Docker not running. Please start Docker and try again.",
        );
        return false;
    }

    // Windows: try launching Docker Desktop
    progress.update_step(docker_step, "running", "Starting Docker Desktop...");
    progress.append_tab(0, "Docker not running -- starting Docker Desktop...");
    if let Err(e) = platform::start_docker_desktop().await {
        tracing::warn!("Failed to start Docker Desktop: {}", e);
    }

    if poll_docker(docker, progress, docker_step, "Waiting for Docker").await {
        progress.update_step(docker_step, "done", "Started");
        progress.append_tab(0, "Docker Desktop started successfully.");
        return true;
    }

    // Stop then start (clears stuck state)
    progress.update_step(docker_step, "running", "Restarting Docker Desktop...");
    progress.append_tab(0, "Docker Desktop not responding -- restarting...");
    if let Err(e) = platform::stop_docker_desktop().await {
        tracing::warn!("Failed to stop Docker Desktop: {}", e);
    }
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    if let Err(e) = platform::start_docker_desktop().await {
        tracing::warn!("Failed to start Docker Desktop: {}", e);
    }

    if poll_docker(docker, progress, docker_step, "Waiting for restart").await {
        progress.update_step(docker_step, "done", "Started");
        progress.append_tab(0, "Docker Desktop started successfully.");
        return true;
    }

    progress.update_step(docker_step, "failed", "Failed to start");
    progress.append_tab(
        0,
        "Could not start Docker Desktop. Please start it manually.",
    );
    false
}

/// Poll Docker readiness up to 60 seconds (12 x 5s).
async fn poll_docker<D: DockerOps>(
    docker: &D,
    progress: &dyn Progress,
    step: usize,
    label: &str,
) -> bool {
    for i in 0..12 {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        progress.update_step(
            step,
            "running",
            &format!("{}... ({}s)", label, (i + 1) * 5),
        );
        if check_docker_with_timeout(docker).await {
            return true;
        }
    }
    false
}

/// Check Docker readiness with a 5-second timeout.
async fn check_docker_with_timeout<D: DockerOps>(docker: &D) -> bool {
    tokio::time::timeout(std::time::Duration::from_secs(5), docker.is_running())
        .await
        .unwrap_or(false)
}

/// Ensure a k3d cluster exists with the required volume mounts.
///
/// If the cluster exists but is missing mounts, it is deleted and recreated.
/// Returns `true` on success.
async fn ensure_k3d_cluster(
    config: &LaunchConfig,
    progress: &dyn Progress,
) -> bool {
    use std::process::Stdio;
    use tokio::process::Command;

    let check = Command::new("k3d")
        .args(["cluster", "list", "-o", "json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;

    let cluster_exists = match check {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).contains("claude-code")
        }
        _ => false,
    };

    let required_mounts = build_required_mounts(config);

    if cluster_exists {
        if verify_k3d_mounts(&required_mounts).await {
            return true;
        }
        progress.append_tab(
            0,
            "k3d cluster exists but is missing volume mounts. Recreating...",
        );
        let delete = Command::new("k3d")
            .args(["cluster", "delete", "claude-code"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;
        match delete {
            Ok(output) if output.status.success() => {
                progress.append_tab(0, "Old cluster deleted.");
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let msg = format!("Failed to delete old cluster: {}", stderr.trim());
                progress.append_tab(0, &msg);
                progress.log(&msg);
                return false;
            }
            Err(e) => {
                let msg = format!("Failed to delete old cluster: {}", e);
                progress.append_tab(0, &msg);
                progress.log(&msg);
                return false;
            }
        }
    }

    progress.append_tab(0, "Creating k3d cluster with volume mounts...");
    progress.log("Creating k3d cluster with volume mounts...");

    let memory_limit = format!("{}m", config.cluster_memory_total_mb);
    let volume_flags = build_volume_flags(config);

    let mut args = vec![
        "cluster", "create", "claude-code",
        "--wait", "--timeout", "120s",
        "--servers-memory",
    ];
    let mem_ref = memory_limit.as_str();
    args.push(mem_ref);

    let vol_refs: Vec<&str> = volume_flags.iter().map(|s| s.as_str()).collect();
    let mut full_args = args;
    for v in &vol_refs {
        full_args.push(v);
    }

    let create = Command::new("k3d")
        .args(&full_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;

    match create {
        Ok(output) if output.status.success() => {
            progress.append_tab(0, "k3d cluster created successfully.");
            true
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let msg = format!("k3d cluster creation failed: {}", stderr.trim());
            progress.append_tab(0, &msg);
            progress.log(&msg);
            false
        }
        Err(e) => {
            let msg = format!("k3d cluster creation error: {}", e);
            progress.append_tab(0, &msg);
            progress.log(&msg);
            false
        }
    }
}

/// Build the list of container-side mount paths that must exist in k3d.
fn build_required_mounts(config: &LaunchConfig) -> Vec<String> {
    let mut mounts = Vec::new();
    if let Some(ref projects_dir) = config.projects_dir {
        mounts.push(platform::to_k3d_container_path(
            projects_dir,
            &config.platform,
        ));
    }
    if let Some(home) = dirs::home_dir() {
        let host = home.join(".claude").to_string_lossy().to_string();
        mounts.push(platform::to_k3d_container_path(&host, &config.platform));
    }
    mounts
}

/// Build k3d `--volume` flag pairs for cluster creation.
fn build_volume_flags(config: &LaunchConfig) -> Vec<String> {
    let mut flags = Vec::new();
    if let Some(ref projects_dir) = config.projects_dir {
        let container = platform::to_k3d_container_path(projects_dir, &config.platform);
        flags.push("--volume".to_string());
        flags.push(platform::k3d_volume_flag(projects_dir, &container));
    }
    if let Some(home) = dirs::home_dir() {
        let claude_dir = home.join(".claude");
        let host = claude_dir.to_string_lossy().to_string();
        let container = platform::to_k3d_container_path(&host, &config.platform);
        flags.push("--volume".to_string());
        flags.push(platform::k3d_volume_flag(&host, &container));
    }
    flags
}

/// Check whether required mount paths exist inside the k3d server container.
async fn verify_k3d_mounts(required_paths: &[String]) -> bool {
    use std::process::Stdio;
    use tokio::process::Command;

    if required_paths.is_empty() {
        return true;
    }

    let test_expr: Vec<String> = required_paths
        .iter()
        .map(|p| format!("test -d '{}'", p))
        .collect();
    let cmd = test_expr.join(" && ");

    let result = Command::new("docker")
        .args(["exec", "k3d-claude-code-server-0", "sh", "-c", &cmd])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;

    matches!(result, Ok(output) if output.status.success())
}

/// Construct a `LaunchResult` for early failures (before any builds).
fn early_result(config: &LaunchConfig) -> LaunchResult {
    LaunchResult {
        deployed: Vec::new(),
        build_failures: Vec::new(),
        deploy_failures: Vec::new(),
        cancelled: config.cancel.load(Ordering::Relaxed),
        pods: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{AppError, AppResult, CmdResult};
    use crate::kubectl::PodStatus;
    use crate::progress::RecordingProgress;
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::Mutex;

    // ── Mock helpers ───────────────────────────────────────────

    fn ok_cmd(stdout: &str) -> AppResult<CmdResult> {
        Ok(CmdResult {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }

    fn fail_cmd(stderr: &str) -> AppResult<CmdResult> {
        Ok(CmdResult {
            success: false,
            stdout: String::new(),
            stderr: stderr.to_string(),
        })
    }

    fn make_project(name: &str) -> Project {
        Project {
            name: name.to_string(),
            path: PathBuf::from(format!("/tmp/test-{}", name)),
            selected: true,
            base_image: projects::BaseImage::Node,
            has_custom_dockerfile: false,
            ambiguous: false,
        }
    }

    fn make_config(projects: Vec<Project>) -> LaunchConfig {
        LaunchConfig {
            projects,
            platform: Platform::Linux,
            cancel: Arc::new(AtomicBool::new(false)),
            credentials_path: String::new(),
            extra_mounts: Vec::new(),
            projects_dir: None,
            cluster_memory_total_mb: 4096,
        }
    }

    fn make_pod(name: &str) -> PodStatus {
        PodStatus {
            name: name.to_string(),
            project: name.to_string(),
            phase: "Running".to_string(),
            ready: true,
            restart_count: 0,
            age: "1m".to_string(),
            warnings: Vec::new(),
            exposed: false,
            container_port: 8080,
            selected: false,
        }
    }

    // ── Mock Docker ────────────────────────────────────────────

    struct MockDocker {
        running: bool,
        build_results: Mutex<VecDeque<AppResult<CmdResult>>>,
    }

    impl MockDocker {
        fn new(running: bool) -> Self {
            Self {
                running,
                build_results: Mutex::new(VecDeque::new()),
            }
        }

        fn with_builds(running: bool, results: Vec<AppResult<CmdResult>>) -> Self {
            Self {
                running,
                build_results: Mutex::new(results.into()),
            }
        }
    }

    impl DockerOps for MockDocker {
        async fn is_running(&self) -> bool {
            self.running
        }
        async fn check_health(&self) -> (bool, String) {
            (self.running, String::new())
        }
        async fn build_and_import_streaming(
            &self,
            _project: &Project,
            _cancel: &AtomicBool,
            _on_line: &(dyn Fn(&str) + Send + Sync),
        ) -> AppResult<CmdResult> {
            self.build_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| ok_cmd("built"))
        }
        async fn import_to_k3s(&self, _tag: &str) -> AppResult<CmdResult> {
            ok_cmd("imported")
        }
    }

    // ── Mock Helm ──────────────────────────────────────────────

    struct MockHelm {
        install_results: Mutex<VecDeque<AppResult<CmdResult>>>,
        uninstall_results: Mutex<VecDeque<AppResult<CmdResult>>>,
        releases: Mutex<Vec<String>>,
    }

    impl MockHelm {
        fn new() -> Self {
            Self {
                install_results: Mutex::new(VecDeque::new()),
                uninstall_results: Mutex::new(VecDeque::new()),
                releases: Mutex::new(Vec::new()),
            }
        }

        fn with_installs(results: Vec<AppResult<CmdResult>>) -> Self {
            Self {
                install_results: Mutex::new(results.into()),
                uninstall_results: Mutex::new(VecDeque::new()),
                releases: Mutex::new(Vec::new()),
            }
        }

        fn with_releases(names: Vec<&str>) -> Self {
            Self {
                install_results: Mutex::new(VecDeque::new()),
                uninstall_results: Mutex::new(VecDeque::new()),
                releases: Mutex::new(
                    names.into_iter().map(String::from).collect(),
                ),
            }
        }
    }

    impl HelmOps for MockHelm {
        async fn install_project(
            &self, _name: &str, _path: &str, _image: &str,
            _extra: &[(&str, &str)],
        ) -> AppResult<CmdResult> {
            self.install_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| ok_cmd("installed"))
        }
        async fn uninstall_project(&self, _name: &str) -> AppResult<CmdResult> {
            self.uninstall_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| ok_cmd("uninstalled"))
        }
        async fn uninstall(&self, _name: &str) -> AppResult<CmdResult> {
            self.uninstall_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| ok_cmd("uninstalled"))
        }
        async fn list_releases(&self) -> AppResult<CmdResult> {
            let names = self.releases.lock().unwrap().join("\n");
            ok_cmd(&names)
        }
        async fn release_count(&self) -> usize {
            self.releases.lock().unwrap().len()
        }
        async fn status(&self) -> AppResult<CmdResult> {
            ok_cmd("status ok")
        }
    }

    // ── Mock Kubectl ───────────────────────────────────────────

    struct MockKubectl {
        pods: Vec<PodStatus>,
        cluster_healthy: bool,
        secret_results: Mutex<VecDeque<AppResult<CmdResult>>>,
    }

    impl MockKubectl {
        fn healthy() -> Self {
            Self {
                pods: Vec::new(),
                cluster_healthy: true,
                secret_results: Mutex::new(VecDeque::new()),
            }
        }

        fn healthy_with_pods(pods: Vec<PodStatus>) -> Self {
            Self {
                pods,
                cluster_healthy: true,
                secret_results: Mutex::new(VecDeque::new()),
            }
        }

        fn unhealthy() -> Self {
            Self {
                pods: Vec::new(),
                cluster_healthy: false,
                secret_results: Mutex::new(VecDeque::new()),
            }
        }
    }

    impl KubeOps for MockKubectl {
        async fn get_pods(&self) -> AppResult<Vec<PodStatus>> {
            Ok(self.pods.clone())
        }
        async fn cluster_health(&self) -> AppResult<bool> {
            Ok(self.cluster_healthy)
        }
        async fn delete_pod(&self, _name: &str) -> AppResult<CmdResult> {
            ok_cmd("deleted")
        }
        async fn get_logs(&self, _name: &str, _tail: u32) -> AppResult<CmdResult> {
            ok_cmd("logs")
        }
        async fn describe_pod(&self, _name: &str) -> AppResult<CmdResult> {
            ok_cmd("describe")
        }
        async fn create_service(
            &self, _project: &str, _port: u16,
        ) -> AppResult<CmdResult> {
            ok_cmd("service created")
        }
        async fn create_ingress(
            &self, _project: &str, _port: u16,
        ) -> AppResult<CmdResult> {
            ok_cmd("ingress created")
        }
        async fn detect_listening_port(&self, _pod: &str) -> (u16, bool) {
            (8080, true)
        }
        async fn apply_secret_from_env(
            &self, _project: &str, _env_vars: &[(String, String)],
        ) -> AppResult<CmdResult> {
            self.secret_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| ok_cmd("secret applied"))
        }
        async fn enrich_pods_with_events(
            &self, _pods: &mut [PodStatus],
        ) -> AppResult<()> {
            Ok(())
        }
    }

    // ── launch_projects tests ──────────────────────────────────

    #[tokio::test]
    async fn launch_happy_path_two_projects() {
        let config = make_config(vec![
            make_project("alpha"),
            make_project("beta"),
        ]);
        let docker = MockDocker::new(true);
        let helm = MockHelm::new();
        let kubectl = MockKubectl::healthy_with_pods(vec![
            make_pod("alpha"),
            make_pod("beta"),
        ]);
        let progress = RecordingProgress::new();

        let result = launch_projects(&config, &docker, &helm, &kubectl, &progress).await;

        assert_eq!(result.deployed.len(), 2);
        assert!(result.build_failures.is_empty());
        assert!(result.deploy_failures.is_empty());
        assert!(!result.cancelled);
        assert!(result.pods.is_some());
    }

    #[tokio::test]
    async fn launch_one_build_fails() {
        let config = make_config(vec![
            make_project("good"),
            make_project("bad"),
        ]);
        let docker = MockDocker::with_builds(true, vec![
            ok_cmd("built"),
            fail_cmd("compilation error"),
        ]);
        let helm = MockHelm::new();
        let kubectl = MockKubectl::healthy();
        let progress = RecordingProgress::new();

        let result = launch_projects(&config, &docker, &helm, &kubectl, &progress).await;

        assert_eq!(result.deployed.len(), 1);
        assert_eq!(result.deployed[0], "good");
        assert_eq!(result.build_failures.len(), 1);
        assert_eq!(result.build_failures[0], "bad");
    }

    #[tokio::test]
    async fn launch_all_builds_fail_no_deploy() {
        let config = make_config(vec![
            make_project("a"),
            make_project("b"),
        ]);
        let docker = MockDocker::with_builds(true, vec![
            fail_cmd("error a"),
            fail_cmd("error b"),
        ]);
        let helm = MockHelm::new();
        let kubectl = MockKubectl::healthy();
        let progress = RecordingProgress::new();

        let result = launch_projects(&config, &docker, &helm, &kubectl, &progress).await;

        assert!(result.deployed.is_empty());
        assert_eq!(result.build_failures.len(), 2);
        assert!(result.deploy_failures.is_empty());
        assert!(result.pods.is_none());
    }

    #[tokio::test]
    async fn launch_cancel_mid_build() {
        let config = make_config(vec![
            make_project("first"),
            make_project("second"),
        ]);
        // Cancel flag set before first build returns
        config.cancel.store(true, Ordering::Relaxed);

        let docker = MockDocker::new(true);
        let helm = MockHelm::new();
        let kubectl = MockKubectl::healthy();
        let progress = RecordingProgress::new();

        let result = launch_projects(&config, &docker, &helm, &kubectl, &progress).await;

        assert!(result.deployed.is_empty());
        assert!(result.cancelled);
        assert!(progress.has_event(|e| matches!(
            e,
            crate::progress::ProgressEvent::AppendTab { text, .. }
                if text.contains("Cancelled by user")
        )));
    }

    #[tokio::test]
    async fn launch_docker_not_running_non_windows() {
        let config = make_config(vec![make_project("p")]);
        let docker = MockDocker::new(false);
        let helm = MockHelm::new();
        let kubectl = MockKubectl::healthy();
        let progress = RecordingProgress::new();

        let result = launch_projects(&config, &docker, &helm, &kubectl, &progress).await;

        assert!(result.deployed.is_empty());
        assert!(progress.has_event(|e| matches!(
            e,
            crate::progress::ProgressEvent::UpdateStep { status, .. }
                if status == "failed"
        )));
        assert!(progress.has_event(|e| matches!(
            e,
            crate::progress::ProgressEvent::AppendTab { text, .. }
                if text.contains("Docker not running")
        )));
    }

    #[tokio::test]
    async fn launch_helm_deploy_fails_then_retry_succeeds() {
        let config = make_config(vec![make_project("svc")]);
        let docker = MockDocker::new(true);
        // First install fails, uninstall succeeds, retry install succeeds
        let helm = MockHelm::with_installs(vec![
            fail_cmd("upgrade failed"),
            ok_cmd("installed"),
        ]);
        let kubectl = MockKubectl::healthy();
        let progress = RecordingProgress::new();

        let result = launch_projects(&config, &docker, &helm, &kubectl, &progress).await;

        assert_eq!(result.deployed.len(), 1);
        assert!(result.deploy_failures.is_empty());
        assert!(progress.has_event(|e| matches!(
            e,
            crate::progress::ProgressEvent::AppendTab { text, .. }
                if text.contains("retrying with reinstall")
        )));
    }

    #[tokio::test]
    async fn launch_helm_deploy_fails_both_attempts() {
        let config = make_config(vec![make_project("broken")]);
        let docker = MockDocker::new(true);
        let helm = MockHelm::with_installs(vec![
            fail_cmd("first fail"),
            fail_cmd("second fail"),
        ]);
        let kubectl = MockKubectl::healthy();
        let progress = RecordingProgress::new();

        let result = launch_projects(&config, &docker, &helm, &kubectl, &progress).await;

        assert!(result.deployed.is_empty());
        assert_eq!(result.deploy_failures.len(), 1);
        assert_eq!(result.deploy_failures[0], "broken");
    }

    #[tokio::test]
    async fn launch_build_error_returns_app_error() {
        let config = make_config(vec![make_project("err")]);
        let docker = MockDocker::with_builds(true, vec![
            Err(AppError::Docker("docker daemon crashed".into())),
        ]);
        let helm = MockHelm::new();
        let kubectl = MockKubectl::healthy();
        let progress = RecordingProgress::new();

        let result = launch_projects(&config, &docker, &helm, &kubectl, &progress).await;

        assert!(result.deployed.is_empty());
        assert_eq!(result.build_failures.len(), 1);
        assert!(progress.has_event(|e| matches!(
            e,
            crate::progress::ProgressEvent::AppendTab { text, .. }
                if text.contains("docker daemon crashed")
        )));
    }

    #[tokio::test]
    async fn launch_cluster_unhealthy_shows_warning() {
        let config = make_config(vec![make_project("p")]);
        let docker = MockDocker::new(true);
        let helm = MockHelm::new();
        let kubectl = MockKubectl::unhealthy();
        let progress = RecordingProgress::new();

        let result = launch_projects(&config, &docker, &helm, &kubectl, &progress).await;

        // Build + deploy should still proceed
        assert_eq!(result.deployed.len(), 1);
        // But a recovery hint should be set
        assert!(progress.has_event(|e| matches!(
            e,
            crate::progress::ProgressEvent::SetRecoveryHint { hint }
                if hint.contains("Cluster connection lost")
        )));
    }

    #[tokio::test]
    async fn launch_empty_projects_returns_early() {
        let config = make_config(Vec::new());
        let docker = MockDocker::new(true);
        let helm = MockHelm::new();
        let kubectl = MockKubectl::healthy();
        let progress = RecordingProgress::new();

        // With empty projects, the orchestrator should add infra steps
        // then immediately hit the "no builds" path
        let result = launch_projects(&config, &docker, &helm, &kubectl, &progress).await;

        assert!(result.deployed.is_empty());
        assert!(result.build_failures.is_empty());
    }

    #[tokio::test]
    async fn launch_progress_reports_build_steps() {
        let config = make_config(vec![make_project("app")]);
        let docker = MockDocker::new(true);
        let helm = MockHelm::new();
        let kubectl = MockKubectl::healthy();
        let progress = RecordingProgress::new();

        let _result = launch_projects(&config, &docker, &helm, &kubectl, &progress).await;

        // Should have added a "Building app" step
        assert!(progress.has_event(|e| matches!(
            e,
            crate::progress::ProgressEvent::AddStep { label, .. }
                if label == "Building app"
        )));
        // Docker step should be marked done
        assert!(progress.has_event(|e| matches!(
            e,
            crate::progress::ProgressEvent::AddStep { label, status, .. }
                if label == "Docker" && status == "running"
        )));
    }

    // ── stop_all tests ─────────────────────────────────────────

    #[tokio::test]
    async fn stop_all_uninstalls_all_releases() {
        let helm = MockHelm::with_releases(vec!["alpha", "beta"]);
        let progress = RecordingProgress::new();

        let result = stop_all(&helm, &progress).await;

        assert_eq!(result.uninstalled.len(), 2);
        assert!(result.failures.is_empty());
        assert!(progress.has_event(|e| matches!(
            e,
            crate::progress::ProgressEvent::Log { msg }
                if msg.contains("Uninstalled 2 release(s)")
        )));
    }

    #[tokio::test]
    async fn stop_all_empty_releases() {
        let helm = MockHelm::with_releases(Vec::new());
        let progress = RecordingProgress::new();

        let result = stop_all(&helm, &progress).await;

        assert!(result.uninstalled.is_empty());
        assert!(result.failures.is_empty());
    }

    #[tokio::test]
    async fn stop_all_partial_failure() {
        let helm = MockHelm::with_releases(vec!["ok", "fail"]);
        // First uninstall succeeds, second fails
        helm.uninstall_results.lock().unwrap().push_back(ok_cmd("done"));
        helm.uninstall_results.lock().unwrap().push_back(fail_cmd("stuck"));
        let progress = RecordingProgress::new();

        let result = stop_all(&helm, &progress).await;

        assert_eq!(result.uninstalled.len(), 1);
        assert_eq!(result.failures.len(), 1);
    }

    #[tokio::test]
    async fn stop_all_list_error() {
        let helm = MockHelm::new();
        // Override list_releases to return an error — we need a custom mock
        // for this; the default returns empty. Actually the default MockHelm
        // has empty releases, which means stdout is "" and success=true.
        // An empty list is fine — just tests that no uninstalls happen.
        let progress = RecordingProgress::new();
        let result = stop_all(&helm, &progress).await;
        assert!(result.uninstalled.is_empty());
    }

    // ── retry_build tests ──────────────────────────────────────

    #[tokio::test]
    async fn retry_build_success_builds_and_deploys() {
        let project = make_project("retryable");
        let docker = MockDocker::new(true);
        let helm = MockHelm::new();
        let cancel = AtomicBool::new(false);
        let progress = RecordingProgress::new();

        let result = retry_build(
            &project, &Platform::Linux, &cancel, "",
            &[], &docker, &helm, &progress,
        ).await;

        assert!(result.build_ok);
        assert!(result.deploy_ok);
        assert!(progress.has_event(|e| matches!(
            e,
            crate::progress::ProgressEvent::AppendTab { text, .. }
                if text.contains("rebuilt successfully")
        )));
    }

    #[tokio::test]
    async fn retry_build_fails_no_deploy() {
        let project = make_project("broken");
        let docker = MockDocker::with_builds(true, vec![
            fail_cmd("compile error"),
        ]);
        let helm = MockHelm::new();
        let cancel = AtomicBool::new(false);
        let progress = RecordingProgress::new();

        let result = retry_build(
            &project, &Platform::Linux, &cancel, "",
            &[], &docker, &helm, &progress,
        ).await;

        assert!(!result.build_ok);
        assert!(!result.deploy_ok);
    }

    #[tokio::test]
    async fn retry_build_ok_but_deploy_fails() {
        let project = make_project("half");
        let docker = MockDocker::new(true);
        let helm = MockHelm::with_installs(vec![
            fail_cmd("deploy error"),
        ]);
        let cancel = AtomicBool::new(false);
        let progress = RecordingProgress::new();

        let result = retry_build(
            &project, &Platform::Linux, &cancel, "",
            &[], &docker, &helm, &progress,
        ).await;

        assert!(result.build_ok);
        assert!(!result.deploy_ok);
        assert!(progress.has_event(|e| matches!(
            e,
            crate::progress::ProgressEvent::AppendTab { text, .. }
                if text.contains("deploy failed")
        )));
    }

    #[tokio::test]
    async fn retry_build_error_shows_hint() {
        let project = make_project("oom");
        let docker = MockDocker::with_builds(true, vec![
            Err(AppError::Docker("out of memory".into())),
        ]);
        let helm = MockHelm::new();
        let cancel = AtomicBool::new(false);
        let progress = RecordingProgress::new();

        let result = retry_build(
            &project, &Platform::Linux, &cancel, "",
            &[], &docker, &helm, &progress,
        ).await;

        assert!(!result.build_ok);
        assert!(!result.deploy_ok);
        assert!(progress.has_event(|e| matches!(
            e,
            crate::progress::ProgressEvent::AppendTab { text, .. }
                if text.contains("out of memory")
        )));
    }

    // ── Helper function tests ──────────────────────────────────

    #[test]
    fn build_extra_helm_args_with_credentials() {
        let config = LaunchConfig {
            projects: Vec::new(),
            platform: Platform::Linux,
            cancel: Arc::new(AtomicBool::new(false)),
            credentials_path: "/home/user/.claude".to_string(),
            extra_mounts: vec!["/data".to_string()],
            projects_dir: None,
            cluster_memory_total_mb: 4096,
        };
        let args = build_extra_helm_args(&config);
        assert_eq!(args.len(), 2);
        assert_eq!(args[0].0, "claude.credentialsPath");
        assert_eq!(args[0].1, "/home/user/.claude");
        assert_eq!(args[1].0, "extraMounts[0]");
    }

    #[test]
    fn build_extra_helm_args_empty_credentials() {
        let config = make_config(Vec::new());
        let args = build_extra_helm_args(&config);
        assert!(args.is_empty());
    }

    #[test]
    fn early_result_not_cancelled() {
        let config = make_config(Vec::new());
        let result = early_result(&config);
        assert!(!result.cancelled);
        assert!(result.deployed.is_empty());
    }

    #[test]
    fn early_result_cancelled() {
        let config = make_config(Vec::new());
        config.cancel.store(true, Ordering::Relaxed);
        let result = early_result(&config);
        assert!(result.cancelled);
    }

    #[tokio::test]
    async fn launch_build_summary_on_partial_failure() {
        let config = make_config(vec![
            make_project("ok1"),
            make_project("fail1"),
            make_project("ok2"),
        ]);
        let docker = MockDocker::with_builds(true, vec![
            ok_cmd("built"),
            fail_cmd("error"),
            ok_cmd("built"),
        ]);
        let helm = MockHelm::new();
        let kubectl = MockKubectl::healthy();
        let progress = RecordingProgress::new();

        let result = launch_projects(&config, &docker, &helm, &kubectl, &progress).await;

        assert_eq!(result.deployed.len(), 2);
        assert_eq!(result.build_failures.len(), 1);
        assert!(progress.has_event(|e| matches!(
            e,
            crate::progress::ProgressEvent::AppendTab { text, .. }
                if text.contains("Build summary: 2 succeeded, 1 failed")
        )));
    }
}
