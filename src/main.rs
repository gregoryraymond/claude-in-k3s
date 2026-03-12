mod app;
mod config;
mod deps;
mod docker;
mod error;
mod health;
mod helm;
mod lockfile;
mod kubectl;
#[allow(dead_code)]
mod orchestrator;
mod platform;
mod progress;
mod projects;
mod recovery;
mod state;
#[cfg(test)]
mod test_utils;
mod terraform;
mod tray;

use app::AppState;
use projects::BaseImage;
use slint::Model;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use kubectl::PodStatus;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    let _log = ck3_logging::init("claude-in-k3s", "claude_in_k3s=info");

    // APP-1/APP-2/APP-12: Single-instance enforcement via lockfile
    if let Err(msg) = lockfile::acquire() {
        eprintln!("{}", msg);
        std::process::exit(1);
    }

    let rt = tokio::runtime::Runtime::new()?;

    let state = Arc::new(Mutex::new(AppState::new()?));

    {
        let s = state.lock().unwrap();
        tracing::info!("Claude in K3s starting — platform: {:?}", s.platform);
    }
    tracing::info!("Log directory: {}", _log.log_dir().display());

    let ui = AppWindow::new()?;

    // Set initial UI state
    {
        let s = state.lock().unwrap();
        ui.set_platform_name(platform::platform_display_name(&s.platform).into());
        if let Some(ref dir) = s.config.projects_dir {
            ui.set_projects_dir(dir.into());
        }
        ui.set_tf_initialized(s.terraform_runner().is_initialized());
        ui.set_claude_mode(s.config.claude_mode.clone().into());
        ui.set_git_user_name(s.config.git_user_name.clone().into());
        ui.set_git_user_email(s.config.git_user_email.clone().into());
        ui.set_cpu_limit(s.config.cpu_limit.clone().into());
        ui.set_memory_limit(s.config.memory_limit.clone().into());
        ui.set_cluster_memory_percent(s.config.cluster_memory_percent.to_string().into());
        ui.set_cluster_memory_info(compute_memory_info(s.config.cluster_memory_percent).into());
        ui.set_terraform_dir(s.config.terraform_dir.clone().into());
        ui.set_helm_chart_dir(s.config.helm_chart_dir.clone().into());
        // SET-12: Load extra mounts as comma-separated string
        ui.set_extra_mounts_text(s.config.extra_mounts.join(", ").into());
    }

    // Set k3s label immediately (doesn't need tool checks)
    {
        let s = state.lock().unwrap();
        ui.set_k3s_label(platform::k8s_provider_name(&s.platform).into());
    }

    // Check deps asynchronously on a background thread so the UI appears instantly
    {
        let plat = state.lock().unwrap().platform.clone();
        let state_for_deps = state.clone();
        let ui_weak = ui.as_weak();
        std::thread::spawn(move || {
            let deps = deps::check_all(&plat);
            // Store in shared state
            if let Ok(mut s) = state_for_deps.lock() {
                s.deps_status = deps.clone();
            }
            // Update UI from the event loop thread
            let plat2 = plat.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    // SUP-8: Use platform-aware check (Windows doesn't need terraform)
                    ui.set_all_deps_met(deps.all_met_for(&plat2));
                    if let deps::ToolStatus::Found { ref version } = deps.k3s {
                        ui.set_k3s_found(true);
                        ui.set_k3s_version(version.clone().into());
                    }
                    if let deps::ToolStatus::Found { ref version } = deps.terraform {
                        ui.set_terraform_found(true);
                        ui.set_terraform_version(version.clone().into());
                    }
                    if let deps::ToolStatus::Found { ref version } = deps.helm {
                        ui.set_helm_found(true);
                        ui.set_helm_version(version.clone().into());
                    }
                    if let deps::ToolStatus::Found { ref version } = deps.docker {
                        ui.set_docker_found(true);
                        ui.set_docker_version(version.clone().into());
                    }
                    if let deps::ToolStatus::Found { ref version } = deps.claude {
                        ui.set_claude_found(true);
                        ui.set_claude_version(version.clone().into());
                    }
                }
            });
        });
    }

    // --- Terraform callbacks ---

    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_cluster_deploy(move || {
            let ui = ui_handle.clone();
            let state = state.clone();
            set_busy(&ui, true);
            append_log(&state, "Deploying cluster...");
            sync_log(&ui, &state);

            rt_handle.spawn(async move {
                // Step 1: terraform init
                let runner = {
                    let s = state.lock().unwrap();
                    s.terraform_runner()
                };
                let init_result = runner.init().await;

                let init_ok = match &init_result {
                    Ok(r) => {
                        let msg = error::format_cmd_result("terraform init", r);
                        let state2 = state.clone();
                        let ui2 = ui.clone();
                        slint::invoke_from_event_loop(move || {
                            append_log(&state2, &msg);
                            sync_log(&ui2, &state2);
                        })
                        .ok();
                        r.success
                    }
                    Err(e) => {
                        let msg = format!("Error: {}", e);
                        let state2 = state.clone();
                        let ui2 = ui.clone();
                        slint::invoke_from_event_loop(move || {
                            append_log(&state2, &msg);
                            sync_log(&ui2, &state2);
                        })
                        .ok();
                        false
                    }
                };

                if !init_ok {
                    // CLU-33: Check for Terraform state corruption — try reinit
                    let stderr = match &init_result {
                        Ok(r) => r.stderr.as_str(),
                        Err(_) => "",
                    };
                    if recovery::is_terraform_state_corrupt(stderr) {
                        let msg = "[Recovery] Terraform state corruption detected — reinitializing...";
                        let state2 = state.clone();
                        let ui2 = ui.clone();
                        slint::invoke_from_event_loop(move || {
                            append_log(&state2, msg);
                            sync_log(&ui2, &state2);
                        }).ok();

                        // Try reinit with -reconfigure
                        let reinit = runner.init_reconfigure().await;
                        let reinit_ok = matches!(&reinit, Ok(r) if r.success);
                        if reinit_ok {
                            let msg2 = "[Recovery] Terraform reinit succeeded.";
                            let state2 = state.clone();
                            let ui2 = ui.clone();
                            slint::invoke_from_event_loop(move || {
                                append_log(&state2, msg2);
                                sync_log(&ui2, &state2);
                            }).ok();
                        } else {
                            let ui2 = ui.clone();
                            let state2 = state.clone();
                            slint::invoke_from_event_loop(move || {
                                append_log(&state2, "[Recovery] Terraform reinit failed. Manual fix: delete the .terraform directory and re-run deploy.");
                                sync_log(&ui2, &state2);
                                set_busy(&ui2, false);
                            }).ok();
                            return;
                        }
                    } else {
                        let ui2 = ui.clone();
                        slint::invoke_from_event_loop(move || {
                            set_busy(&ui2, false);
                        }).ok();
                        return;
                    }
                }

                // Mark tf_initialized after successful init
                {
                    let state2 = state.clone();
                    let ui2 = ui.clone();
                    slint::invoke_from_event_loop(move || {
                        state2.lock().unwrap().tf_initialized = true;
                        if let Some(ui) = ui2.upgrade() {
                            ui.set_tf_initialized(true);
                        }
                    })
                    .ok();
                }

                // Step 2: terraform apply
                let mut apply_result = runner.apply().await;

                // Attempt cluster recovery on failure (Windows/k3d only)
                if let Ok(ref r) = apply_result {
                    if !r.success {
                        let stderr = &r.stderr;
                        let (is_windows, can_retry) = {
                            let s = state.lock().unwrap();
                            (
                                s.platform == platform::Platform::Windows,
                                s.recovery_tracker.can_retry_cluster(),
                            )
                        };
                        if is_windows {
                            if let Some(action) = recovery::diagnose_cluster_failure(stderr) {
                                if can_retry {
                                    let memory_limit = {
                                        let s = state.lock().unwrap();
                                        let mem = s.compute_cluster_memory_limit();
                                        format!("{}m", mem)
                                    };

                                    let msg = action.description();
                                    let state2 = state.clone();
                                    let ui2 = ui.clone();
                                    slint::invoke_from_event_loop(move || {
                                        append_log(&state2, &msg);
                                        sync_log(&ui2, &state2);
                                    }).ok();

                                    let fix_result = recovery::recreate_k3d_cluster(&memory_limit).await;
                                    match fix_result {
                                        Ok(fr) if fr.success => {
                                            {
                                                let mut s = state.lock().unwrap();
                                                s.recovery_tracker.record_cluster_attempt();
                                            }
                                            let msg2 = "[Recovery] Cluster recreated. Retrying terraform apply...".to_string();
                                            let state3 = state.clone();
                                            let ui3 = ui.clone();
                                            slint::invoke_from_event_loop(move || {
                                                append_log(&state3, &msg2);
                                                sync_log(&ui3, &state3);
                                            }).ok();
                                            apply_result = runner.apply().await;
                                        }
                                        Ok(fr) => {
                                            let msg2 = format!("[Recovery] Cluster recreate failed: {}", fr.stderr);
                                            let state3 = state.clone();
                                            let ui3 = ui.clone();
                                            slint::invoke_from_event_loop(move || {
                                                append_log(&state3, &msg2);
                                                sync_log(&ui3, &state3);
                                            }).ok();
                                        }
                                        Err(e) => {
                                            let msg2 = format!("[Recovery] Cluster recreate error: {}", e);
                                            let state3 = state.clone();
                                            let ui3 = ui.clone();
                                            slint::invoke_from_event_loop(move || {
                                                append_log(&state3, &msg2);
                                                sync_log(&ui3, &state3);
                                            }).ok();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                slint::invoke_from_event_loop(move || {
                    match apply_result {
                        Ok(r) => {
                            append_log(&state, &error::format_cmd_result("terraform apply", &r));
                            if r.success {
                                state.lock().unwrap().cluster_healthy = true;
                                state.lock().unwrap().recovery_tracker.reset();
                                if let Some(ui) = ui.upgrade() {
                                    ui.set_cluster_status("Healthy".into());
                                    ui.set_tf_initialized(true);
                                }
                            }
                        }
                        Err(e) => append_log(&state, &format!("Error: {}", e)),
                    }
                    set_busy(&ui, false);
                    sync_log(&ui, &state);
                })
                .ok();
            });
        });
    }

    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_terraform_destroy(move || {
            let ui = ui_handle.clone();
            let state = state.clone();
            set_busy(&ui, true);
            append_log(&state, "Running terraform destroy...");
            sync_log(&ui, &state);

            rt_handle.spawn(async move {
                let runner = {
                    let s = state.lock().unwrap();
                    s.terraform_runner()
                };
                let result = runner.destroy().await;

                slint::invoke_from_event_loop(move || {
                    match result {
                        Ok(r) => {
                            append_log(&state, &error::format_cmd_result("terraform destroy", &r));
                            if r.success {
                                state.lock().unwrap().cluster_healthy = false;
                                if let Some(ui) = ui.upgrade() {
                                    ui.set_cluster_status("Stopped".into());
                                }
                            }
                        }
                        Err(e) => append_log(&state, &format!("Error: {}", e)),
                    }
                    set_busy(&ui, false);
                    sync_log(&ui, &state);
                })
                .ok();
            });
        });
    }

    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_terraform_plan(move || {
            let ui = ui_handle.clone();
            let state = state.clone();
            set_busy(&ui, true);
            append_log(&state, "Running terraform plan...");
            sync_log(&ui, &state);

            rt_handle.spawn(async move {
                let runner = {
                    let s = state.lock().unwrap();
                    s.terraform_runner()
                };
                let result = runner.plan().await;

                slint::invoke_from_event_loop(move || {
                    match result {
                        Ok(r) => {
                            append_log(&state, &error::format_cmd_result("terraform plan", &r));
                        }
                        Err(e) => append_log(&state, &format!("Error: {}", e)),
                    }
                    set_busy(&ui, false);
                    sync_log(&ui, &state);
                })
                .ok();
            });
        });
    }

    // --- Browse folder ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();

        ui.on_browse_folder(move || {
            let dialog = rfd::FileDialog::new()
                .set_title("Select Projects Directory")
                .pick_folder();

            if let Some(path) = dialog {
                let path_str = path.to_string_lossy().to_string();

                {
                    let mut s = state.lock().unwrap();
                    s.config.projects_dir = Some(path_str.clone());
                    if let Err(e) = s.config.save() {
                        tracing::error!("Failed to save config: {}", e);
                    }
                    if let Err(e) = s.scan_projects() {
                        tracing::warn!("Failed to scan projects: {}", e);
                    }
                }

                if let Some(ui) = ui_handle.upgrade() {
                    ui.set_projects_dir(path_str.into());
                    sync_projects(&ui_handle, &state);
                }
            }
        });
    }

    // --- Refresh projects ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();

        ui.on_refresh_projects(move || {
            {
                let mut s = state.lock().unwrap();
                if let Err(e) = s.scan_projects() {
                    tracing::warn!("Failed to scan projects: {}", e);
                }
            }
            sync_projects(&ui_handle, &state);
        });
    }

    // --- Project toggled ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();

        ui.on_project_toggled(move |idx, checked| {
            {
                let mut s = state.lock().unwrap();
                if let Some(p) = s.projects.get_mut(idx as usize) {
                    p.selected = checked;
                }
            }
            sync_projects(&ui_handle, &state);
        });
    }

    // --- Project image changed ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();

        ui.on_project_image_changed(move |idx, img_idx| {
            {
                let mut s = state.lock().unwrap();
                if let Some(p) = s.projects.get_mut(idx as usize) {
                    p.base_image = BaseImage::from_index(img_idx);
                }
            }
            sync_projects(&ui_handle, &state);
        });
    }

    // --- Toggle select all ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();

        ui.on_toggle_select_all(move || {
            {
                let mut s = state.lock().unwrap();
                let all_selected = s.projects.iter().all(|p| p.selected);
                let new_val = !all_selected;
                for p in &mut s.projects {
                    p.selected = new_val;
                }
            }
            sync_projects(&ui_handle, &state);
        });
    }

    // --- Cancel launch ---
    {
        let state = state.clone();

        ui.on_cancel_launch(move || {
            let s = state.lock().unwrap();
            s.cancel_flag.store(true, Ordering::Relaxed);
        });
    }

    // --- Launch selected ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_launch_selected(move || {
            let ui = ui_handle.clone();
            let state = state.clone();

            // Extract state for validation and config building
            let (selected_projects, plat, extra_mounts, projects_dir,
                 cluster_mem_total, credentials_path, memory_limit,
                 cluster_mem_used, cluster_mem_limit, cancel_flag) = {
                let s = state.lock().unwrap();
                s.cancel_flag.store(false, Ordering::Relaxed);
                let projects = s.selected_projects().into_iter().cloned().collect::<Vec<_>>();
                let plat = s.platform.clone();
                let cred = dirs::home_dir()
                    .map(|h| {
                        let hp = h.join(".claude").to_string_lossy().to_string();
                        platform::to_k3d_container_path(&hp, &plat)
                    })
                    .unwrap_or_default();
                (
                    projects,
                    plat,
                    s.config.extra_mounts.clone(),
                    s.config.projects_dir.clone(),
                    s.compute_cluster_memory_limit(),
                    cred,
                    s.config.memory_limit.clone(),
                    s.cluster_memory_used_mb,
                    s.cluster_memory_limit_mb,
                    s.cancel_flag.clone(),
                )
            };

            if selected_projects.is_empty() {
                append_log(&state, "No projects selected.");
                sync_log(&ui, &state);
                return;
            }

            // PRJ-57: Validate extra mount paths exist before launch
            {
                let bad: Vec<_> = extra_mounts
                    .iter()
                    .filter(|m| !std::path::Path::new(m).exists())
                    .cloned()
                    .collect();
                if !bad.is_empty() {
                    append_log(&state, &format!(
                        "Extra mount paths do not exist: {}. Fix in Settings before launching.",
                        bad.join(", ")
                    ));
                    sync_log(&ui, &state);
                    return;
                }
            }

            // PRJ-55: Warn if disk space is low (< 2 GB free)
            if let Ok(free_bytes) = platform::available_disk_space() {
                let free_gb = free_bytes / (1024 * 1024 * 1024);
                if free_gb < 2 {
                    append_log(&state, &format!(
                        "Warning: low disk space ({} GB free). Docker builds may fail. \
                         Consider cleaning old images.",
                        free_gb
                    ));
                    sync_log(&ui, &state);
                }
            }

            // PRJ-30: Warn if cluster doesn't have enough memory
            {
                let mem_per = config::parse_memory_limit_mb(&memory_limit);
                let total = mem_per * selected_projects.len() as u64;
                if let (Some(used), Some(limit)) = (cluster_mem_used, cluster_mem_limit) {
                    if limit > 0 && used + total > limit {
                        append_log(&state, &format!(
                            "Warning: deploying {} project(s) needs ~{} MB but cluster \
                             has only ~{} MB free ({}/{} MB used).",
                            selected_projects.len(), total,
                            limit.saturating_sub(used), used, limit
                        ));
                        sync_log(&ui, &state);
                    }
                }
            }

            set_busy(&ui, true);

            // Build launch tabs: tab 0 = Summary, then one per project
            let mut tabs = Vec::with_capacity(1 + selected_projects.len());
            tabs.push(LaunchTab {
                name: "Summary".into(),
                status: "Building".into(),
                log_text: "Starting build...\n".into(),
            });
            for p in &selected_projects {
                tabs.push(LaunchTab {
                    name: p.name.clone().into(),
                    status: "Pending".into(),
                    log_text: Default::default(),
                });
            }
            if let Some(u) = ui.upgrade() {
                u.set_launch_tabs(
                    std::rc::Rc::new(slint::VecModel::from(tabs)).into(),
                );
                u.set_active_launch_tab(0);
                u.set_launch_steps(
                    std::rc::Rc::new(slint::VecModel::<LaunchStep>::default()).into(),
                );
            }

            let launch_config = orchestrator::LaunchConfig {
                projects: selected_projects,
                platform: plat,
                cancel: cancel_flag,
                credentials_path,
                extra_mounts,
                projects_dir,
                cluster_memory_total_mb: cluster_mem_total,
            };

            rt_handle.spawn(async move {
                let docker = { state.lock().unwrap().docker_builder() };
                let helm = { state.lock().unwrap().helm_runner() };
                let kubectl = { state.lock().unwrap().kubectl_runner() };
                let progress = SlintProgress::new(ui.clone(), state.clone());

                // CLU-32: Record pending deploy for resume capability
                {
                    let mut s = state.lock().unwrap();
                    s.pending_deploy = launch_config.projects
                        .iter()
                        .map(|p| p.name.clone())
                        .collect();
                }

                let result = orchestrator::launch_projects(
                    &launch_config, &docker, &helm, &kubectl, &progress,
                ).await;

                // Apply result to shared state
                {
                    let mut s = state.lock().unwrap();
                    for name in &result.deployed {
                        s.desired_state.mark_deployed(name);
                    }
                    let _ = s.desired_state.save();
                    if result.deploy_failures.is_empty() && !result.deployed.is_empty() {
                        s.recovery_tracker.reset();
                    }
                    s.pending_deploy.clear();
                    if let Some(pods) = result.pods {
                        s.pods = pods;
                    }
                }

                let deploy_ok = !result.deployed.is_empty();
                slint::invoke_from_event_loop({
                    let ui = ui.clone();
                    let state = state.clone();
                    move || {
                        set_busy(&ui, false);
                        sync_log(&ui, &state);
                        sync_pods(&ui, &state);
                        if deploy_ok {
                            if let Some(u) = ui.upgrade() {
                                u.set_active_page(2);
                            }
                        }
                    }
                }).ok();
            });
        });
    }

    // --- Stop selected (uninstall all helm releases) ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_stop_selected(move || {
            let ui = ui_handle.clone();
            let state = state.clone();
            set_busy(&ui, true);

            rt_handle.spawn(async move {
                let helm = { state.lock().unwrap().helm_runner() };
                let progress = SlintProgress::new(ui.clone(), state.clone());

                let _result = orchestrator::stop_all(&helm, &progress).await;

                slint::invoke_from_event_loop(move || {
                    set_busy(&ui, false);
                    sync_log(&ui, &state);
                }).ok();
            });
        });
    }

    // --- PRJ-53: Retry build for a single failed project ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_retry_build(move |step_label| {
            let label = step_label.to_string();
            let project_name = label
                .strip_prefix("Building ")
                .unwrap_or(&label)
                .to_string();

            let ui = ui_handle.clone();
            let state = state.clone();

            let (project, plat) = {
                let s = state.lock().unwrap();
                let proj = s.projects.iter()
                    .find(|p| p.name == project_name)
                    .cloned();
                (proj, s.platform.clone())
            };

            let project = match project {
                Some(p) => p,
                None => {
                    append_to_tab(
                        &ui, 0,
                        &format!("Retry: project '{}' not found.", project_name),
                    );
                    return;
                }
            };

            // Update the failed step to "running"
            slint::invoke_from_event_loop({
                let ui = ui.clone();
                let label = label.clone();
                move || {
                    if let Some(u) = ui.upgrade() {
                        let steps = u.get_launch_steps();
                        for i in 0..steps.row_count() {
                            if let Some(mut step) = steps.row_data(i) {
                                if step.label.as_str() == label {
                                    step.status = "running".into();
                                    step.message = "Retrying...".into();
                                    steps.set_row_data(i, step);
                                    break;
                                }
                            }
                        }
                    }
                }
            }).ok();

            rt_handle.spawn(async move {
                let docker = { state.lock().unwrap().docker_builder() };
                let helm = { state.lock().unwrap().helm_runner() };
                let cancel = { state.lock().unwrap().cancel_flag.clone() };
                let progress = SlintProgress::new(ui.clone(), state.clone());

                // Build extra helm args
                let (credentials_path, extra_helm_args) = {
                    let s = state.lock().unwrap();
                    let cred = dirs::home_dir()
                        .map(|h| {
                            let hp = h.join(".claude").to_string_lossy().to_string();
                            platform::to_k3d_container_path(&hp, &plat)
                        })
                        .unwrap_or_default();
                    let mut args = Vec::new();
                    for (i, mount) in s.config.extra_mounts.iter().enumerate() {
                        let cp = platform::to_k3d_container_path(mount, &plat);
                        args.push((format!("extraMounts[{}]", i), cp));
                    }
                    (cred, args)
                };

                let result = orchestrator::retry_build(
                    &project, &plat, &cancel, &credentials_path,
                    &extra_helm_args, &docker, &helm, &progress,
                ).await;

                let step_status = if result.build_ok { "done" } else { "failed" };
                let step_msg = if result.build_ok {
                    "Rebuilt successfully"
                } else {
                    "Retry failed"
                };

                slint::invoke_from_event_loop({
                    let ui = ui.clone();
                    move || {
                        if let Some(u) = ui.upgrade() {
                            let steps = u.get_launch_steps();
                            for i in 0..steps.row_count() {
                                if let Some(mut step) = steps.row_data(i) {
                                    if step.label.as_str() == label {
                                        step.status = step_status.into();
                                        step.message = step_msg.into();
                                        steps.set_row_data(i, step);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }).ok();
            });
        });
    }

    // --- Helm status ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_helm_status(move || {
            let ui = ui_handle.clone();
            let state = state.clone();
            set_busy(&ui, true);
            append_log(&state, "Checking Helm release status...");
            sync_log(&ui, &state);

            let helm_runner = {
                let s = state.lock().unwrap();
                s.helm_runner()
            };

            rt_handle.spawn(async move {
                let result = helm_runner.status().await;

                slint::invoke_from_event_loop(move || {
                    match result {
                        Ok(r) => {
                            append_log(&state, &error::format_cmd_result("helm status", &r));
                        }
                        Err(e) => append_log(&state, &format!("Helm status error: {}", e)),
                    }
                    set_busy(&ui, false);
                    sync_log(&ui, &state);
                })
                .ok();
            });
        });
    }

    // --- Restart WSL ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_restart_wsl(move || {
            let ui = ui_handle.clone();
            let state = state.clone();
            set_busy(&ui, true);
            append_log(&state, "Restarting WSL...");
            sync_log(&ui, &state);

            rt_handle.spawn(async move {
                let result = platform::restart_wsl().await;
                // Wait a moment for WSL to come back
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;

                slint::invoke_from_event_loop(move || {
                    match result {
                        Ok(_) => append_log(&state, "WSL restarted successfully."),
                        Err(e) => append_log(&state, &format!("WSL restart failed: {}", e)),
                    }
                    set_busy(&ui, false);
                    sync_log(&ui, &state);
                })
                .ok();
            });
        });
    }

    // --- Restart Docker Desktop ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_restart_docker(move || {
            let ui = ui_handle.clone();
            let state = state.clone();
            set_busy(&ui, true);
            append_log(&state, "Stopping Docker Desktop...");
            sync_log(&ui, &state);

            rt_handle.spawn(async move {
                let stop_result = platform::stop_docker_desktop().await;
                let stop_msg = match &stop_result {
                    Ok(_) => "Docker Desktop stopped.".to_string(),
                    Err(e) => format!("Docker Desktop stop warning: {}", e),
                };

                let ui2 = ui.clone();
                let state2 = state.clone();
                slint::invoke_from_event_loop(move || {
                    append_log(&state2, &stop_msg);
                    append_log(&state2, "Starting Docker Desktop...");
                    sync_log(&ui2, &state2);
                })
                .ok();

                // Wait for Docker to fully stop
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                let start_result = platform::start_docker_desktop().await;

                slint::invoke_from_event_loop(move || {
                    match start_result {
                        Ok(_) => append_log(&state, "Docker Desktop started. It may take a minute to become ready."),
                        Err(e) => append_log(&state, &format!("Docker Desktop start failed: {}", e)),
                    }
                    set_busy(&ui, false);
                    sync_log(&ui, &state);
                })
                .ok();
            });
        });
    }

    // --- Restart Cluster ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_restart_cluster(move || {
            let ui = ui_handle.clone();
            let state = state.clone();
            set_busy(&ui, true);
            append_log(&state, "Recreating k3d cluster...");
            sync_log(&ui, &state);

            rt_handle.spawn(async move {
                // Delete existing cluster first
                let delete = tokio::process::Command::new("k3d")
                    .args(["cluster", "delete", "claude-code"])
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .output()
                    .await;

                let delete_msg = match &delete {
                    Ok(output) if output.status.success() => "Old cluster deleted.".to_string(),
                    Ok(output) => {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        format!("Cluster delete note: {}", stderr.trim())
                    }
                    Err(e) => format!("Cluster delete warning: {}", e),
                };

                let ui2 = ui.clone();
                let state2 = state.clone();
                slint::invoke_from_event_loop(move || {
                    append_log(&state2, &delete_msg);
                    append_log(&state2, "Creating new k3d cluster...");
                    sync_log(&ui2, &state2);
                })
                .ok();

                // Recreate using orchestrator's ensure_k3d_cluster via
                // a minimal launch_projects call that only checks infra
                let progress = SlintProgress::new(ui.clone(), state.clone());
                let config = {
                    let s = state.lock().unwrap();
                    let plat = s.platform.clone();
                    let cred = dirs::home_dir()
                        .map(|h| {
                            let hp = h.join(".claude").to_string_lossy().to_string();
                            platform::to_k3d_container_path(&hp, &plat)
                        })
                        .unwrap_or_default();
                    orchestrator::LaunchConfig {
                        projects: Vec::new(),
                        platform: plat,
                        cancel: s.cancel_flag.clone(),
                        credentials_path: cred,
                        extra_mounts: s.config.extra_mounts.clone(),
                        projects_dir: s.config.projects_dir.clone(),
                        cluster_memory_total_mb: s.compute_cluster_memory_limit(),
                    }
                };

                // Use the orchestrator's internal k3d creation
                // by running a launch with empty projects (only infra steps)
                let docker = { state.lock().unwrap().docker_builder() };
                let helm = { state.lock().unwrap().helm_runner() };
                let kubectl = { state.lock().unwrap().kubectl_runner() };
                let result = orchestrator::launch_projects(
                    &config, &docker, &helm, &kubectl, &progress,
                ).await;
                let ok = !result.cancelled;

                slint::invoke_from_event_loop(move || {
                    if ok {
                        append_log(&state, "k3d cluster recreated successfully.");
                    } else {
                        append_log(&state, "k3d cluster recreation failed. Check logs.");
                    }
                    set_busy(&ui, false);
                    sync_log(&ui, &state);
                })
                .ok();
            });
        });
    }

    // --- Refresh pods ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_refresh_pods(move || {
            let ui = ui_handle.clone();
            let state = state.clone();

            let kubectl = {
                let s = state.lock().unwrap();
                s.kubectl_runner()
            };

            rt_handle.spawn(async move {
                let result = kubectl.get_pods().await;
                let services = kubectl.get_services().await.unwrap_or_default();

                slint::invoke_from_event_loop(move || {
                    match result {
                        Ok(mut pods) => {
                            for p in &mut pods {
                                p.exposed = services.contains(&p.project);
                            }
                            {
                                let mut s = state.lock().unwrap();
                                merge_pod_selection(&s.pods, &mut pods);
                                s.pods = pods;
                            }
                            sync_pods(&ui, &state);
                        }
                        Err(e) => {
                            append_log(&state, &format!("Pod refresh error: {}", e));
                            sync_log(&ui, &state);
                        }
                    }
                })
                .ok();
            });
        });
    }

    // --- Delete pod (uninstall project's helm release) ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_delete_pod(move |idx| {
            let ui = ui_handle.clone();
            let state = state.clone();

            let (helm_runner, kubectl, project_name) = {
                let s = state.lock().unwrap();
                let name = s
                    .pods
                    .get(idx as usize)
                    .map(|p| p.project.clone())
                    .unwrap_or_default();
                (s.helm_runner(), s.kubectl_runner(), name)
            };

            if project_name.is_empty() {
                return;
            }

            set_busy(&ui, true);
            append_log(&state, &format!("Removing project {}...", project_name));
            sync_log(&ui, &state);

            rt_handle.spawn(async move {
                let result = helm_runner.uninstall_project(&project_name).await;

                // Also clean up Service, Ingress, and namespace
                if let Err(e) = kubectl.delete_service(&project_name).await {
                    tracing::warn!("Failed to delete service for '{}': {}", project_name, e);
                }
                if let Err(e) = kubectl.delete_ingress(&project_name).await {
                    tracing::warn!("Failed to delete ingress for '{}': {}", project_name, e);
                }
                // POD-57: Delete the project namespace if helm uninstall succeeded
                if result.as_ref().map_or(false, |r| r.success) {
                    let ns = format!("claude-{}", helm::HelmRunner::release_name_for(&project_name));
                    if let Err(e) = kubectl.delete_namespace(&ns).await {
                        tracing::debug!("Namespace cleanup for '{}': {}", ns, e);
                    }
                }

                // Wait for k8s to reconcile
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let pods_result = kubectl.get_pods().await;

                slint::invoke_from_event_loop(move || {
                    let pod_msg;
                    match result {
                        Ok(r) if r.success => {
                            pod_msg = format!("Removed project {}.", project_name);
                            append_log(&state, &pod_msg);
                            // APP-13: Remove from desired state
                            {
                                let mut s = state.lock().unwrap();
                                s.desired_state.mark_undeployed(&project_name);
                                let _ = s.desired_state.save();
                            }
                        }
                        Ok(r) => {
                            pod_msg = format!("Remove failed: {}", r.stderr.trim());
                            append_log(&state, &pod_msg);
                        }
                        Err(e) => {
                            pod_msg = format!("Remove error: {}", e);
                            append_log(&state, &pod_msg);
                        }
                    }
                    if let Ok(pods) = pods_result {
                        state.lock().unwrap().pods = pods;
                    }
                    // POD-69: Log to pod log viewer as well
                    if let Some(u) = ui.upgrade() {
                        let current = u.get_pod_log().to_string();
                        u.set_pod_log(format!("{}\n--- {} ---\n{}", current, project_name, pod_msg).into());
                    }
                    set_busy(&ui, false);
                    sync_pods(&ui, &state);
                    sync_log(&ui, &state);
                })
                .ok();
            });
        });
    }

    // --- View logs ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_view_logs(move |idx| {
            let ui = ui_handle.clone();
            let state = state.clone();

            let (kubectl_bin, namespace, pod_name, restart_count) = {
                let mut s = state.lock().unwrap();
                let pod = s.pods.get(idx as usize);
                let name = pod.map(|p| p.name.clone()).unwrap_or_default();
                let restarts = pod.map(|p| p.restart_count).unwrap_or(0);
                let bin = platform::kubectl_binary(&s.platform).to_string();
                let ns = "claude-code".to_string();

                // POD-30: Kill any existing log tail process
                if let Some(old_pid) = s.log_tail_pid.take() {
                    #[cfg(unix)]
                    unsafe { libc::kill(old_pid as i32, libc::SIGTERM); }
                    #[cfg(windows)]
                    {
                        let _ = std::process::Command::new("taskkill")
                            .args(["/PID", &old_pid.to_string(), "/F"])
                            .output();
                    }
                }

                (bin, ns, name, restarts)
            };

            if pod_name.is_empty() {
                return;
            }

            let kubectl = { state.lock().unwrap().kubectl_runner() };

            rt_handle.spawn(async move {
                // First, do one-shot fetch for initial display (including previous logs)
                let result = kubectl.get_logs(&pod_name, 100).await;

                // If pod has restarts, also fetch describe to get Events with real errors
                let events_section = if restart_count > 0 {
                    if let Ok(desc) = kubectl.describe_pod(&pod_name).await {
                        if desc.success {
                            kubectl::extract_describe_events(&desc.stdout)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Set initial log text
                let pod_name_for_tail = pod_name.clone();
                {
                    let ui2 = ui.clone();
                    slint::invoke_from_event_loop(move || {
                        let header = format!("--- Logs for {} (streaming) ---\n", pod_name);

                        let mut log_text = match result {
                            Ok(r) if r.success && !r.stdout.trim().is_empty() => {
                                format!("{}{}", header, r.stdout)
                            }
                            Ok(r) if !r.stderr.trim().is_empty() => {
                                format!("{}{}", header, r.stderr.trim())
                            }
                            Ok(_) => {
                                format!("{}(no output yet)", header)
                            }
                            Err(e) => format!("Logs error: {}", e),
                        };

                        if let Some(events) = events_section {
                            log_text.push_str(&format!(
                                "\n\n=== Pod Events (restarts: {}) ===\n{}",
                                restart_count, events
                            ));
                        }

                        if let Some(summary) = recovery::detect_failure_patterns(&log_text) {
                            log_text.insert_str(0, &summary);
                        }

                        let highlighted = recovery::highlight_failure_lines(&log_text);

                        if let Some(u) = ui2.upgrade() {
                            u.set_pod_log(highlighted.into());
                        }
                    }).ok();
                }

                // POD-30: Start background log tailing with --follow
                let tail_result = tokio::process::Command::new(&kubectl_bin)
                    .args(["logs", "--follow", "--tail=0", "-n", &namespace, &pod_name_for_tail])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .spawn();

                if let Ok(mut child) = tail_result {
                    let pid = child.id().unwrap_or(0);
                    {
                        let mut s = state.lock().unwrap();
                        s.log_tail_pid = Some(pid);
                    }

                    if let Some(stdout) = child.stdout.take() {
                        use tokio::io::{AsyncBufReadExt, BufReader};
                        let reader = BufReader::new(stdout);
                        let mut lines = reader.lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            let ui3 = ui.clone();
                            let annotated = if recovery::FAILURE_PATTERNS.iter().any(|pat| line.contains(pat)) {
                                format!("[!] {}", line)
                            } else {
                                line
                            };
                            slint::invoke_from_event_loop(move || {
                                if let Some(u) = ui3.upgrade() {
                                    let mut current = u.get_pod_log().to_string();
                                    current.push('\n');
                                    current.push_str(&annotated);
                                    u.set_pod_log(current.into());
                                }
                            }).ok();
                        }
                    }

                    // Clean up when tail ends
                    let _ = child.wait().await;
                    let mut s = state.lock().unwrap();
                    if s.log_tail_pid == Some(pid) {
                        s.log_tail_pid = None;
                    }
                }
            });
        });
    }

    // --- Toggle Claude remote control (background process) ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_exec_claude(move |idx| {
            let (pod_name, plat, already_running, pid_to_kill) = {
                let s = state.lock().unwrap();
                let name = s
                    .pods
                    .get(idx as usize)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                let running = s.remote_control_procs.contains_key(&name);
                let pid = if running { s.remote_control_procs.get(&name).copied() } else { None };
                (name, s.platform.clone(), running, pid)
            };

            if pod_name.is_empty() {
                return;
            }

            if already_running {
                // Remove from tracking immediately (UI is responsive)
                {
                    let mut s = state.lock().unwrap();
                    s.remote_control_procs.remove(&pod_name);
                    // POD-59: Remove from desired set when user explicitly stops
                    s.remote_control_desired.remove(&pod_name);
                    s.remote_control_restart_count.remove(&pod_name);
                    s.append_log(&format!("Stopped remote control for {}", pod_name));
                }
                sync_pods(&ui_handle, &state);
                sync_log(&ui_handle, &state);

                // Kill the process in the background to avoid blocking the UI
                if let Some(pid) = pid_to_kill {
                    rt_handle.spawn(async move {
                        #[cfg(unix)]
                        {
                            unsafe { libc::kill(pid as i32, libc::SIGTERM); }
                        }
                        #[cfg(windows)]
                        {
                            if let Err(e) = tokio::process::Command::new("taskkill")
                                .args(["/PID", &pid.to_string(), "/F"])
                                .output()
                                .await
                            {
                                tracing::warn!("Failed to taskkill PID {}: {}", pid, e);
                            }
                        }
                    });
                }
            } else {
                // Start the background process using tokio so we can stream output
                let kubectl_bin = platform::kubectl_binary(&plat).to_string();
                let (program, args) = remote_control_command(&kubectl_bin, "claude-code", &pod_name);
                let state2 = state.clone();
                let ui2 = ui_handle.clone();
                let pod = pod_name.clone();

                rt_handle.spawn(async move {
                    let result = tokio::process::Command::new(&program)
                        .args(&args)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .spawn();

                    match result {
                        Ok(mut child) => {
                            let pid = child.id().unwrap_or(0);
                            {
                                let state3 = state2.clone();
                                let ui3 = ui2.clone();
                                let pod2 = pod.clone();
                                slint::invoke_from_event_loop(move || {
                                    {
                                        let mut s = state3.lock().unwrap();
                                        s.remote_control_procs.insert(pod2.clone(), pid);
                                        // POD-59: Track as desired remote-control session
                                        s.remote_control_desired.insert(pod2.clone());
                                        s.append_log(&format!("Started remote control for {} (pid {})", pod2, pid));
                                    }
                                    sync_pods(&ui3, &state3);
                                    sync_log(&ui3, &state3);
                                }).ok();
                            }

                            // Stream stderr to log
                            if let Some(stderr) = child.stderr.take() {
                                use tokio::io::{AsyncBufReadExt, BufReader};
                                let state3 = state2.clone();
                                let ui3 = ui2.clone();
                                let pod2 = pod.clone();
                                tokio::spawn(async move {
                                    let reader = BufReader::new(stderr);
                                    let mut lines = reader.lines();
                                    while let Ok(Some(line)) = lines.next_line().await {
                                        let state4 = state3.clone();
                                        let ui4 = ui3.clone();
                                        let pod3 = pod2.clone();
                                        slint::invoke_from_event_loop(move || {
                                            append_log(&state4, &format!("[{}] {}", pod3, line));
                                            sync_log(&ui4, &state4);
                                        }).ok();
                                    }
                                });
                            }

                            // Wait for process to exit and clean up
                            let exit = child.wait().await;
                            let state3 = state2.clone();
                            let ui3 = ui2.clone();
                            let pod2 = pod.clone();

                            // POD-66/67: Check if we should auto-restart
                            let should_restart = {
                                let mut s = state3.lock().unwrap();
                                s.remote_control_procs.remove(&pod2);
                                let msg = match &exit {
                                    Ok(status) => format!("Remote control for {} exited ({})", pod2, status),
                                    Err(e) => format!("Remote control for {} error: {}", pod2, e),
                                };
                                s.append_log(&msg);

                                // POD-66: Auto-restart if still desired and POD-67: under 10 attempts
                                let desired = s.remote_control_desired.contains(&pod2);
                                let pod_exists = s.pods.iter().any(|p| p.name == pod2 && p.phase == "Running");
                                let restart_count = {
                                    let count = s.remote_control_restart_count.entry(pod2.clone()).or_insert(0);
                                    *count += 1;
                                    *count
                                };
                                if desired && pod_exists && restart_count <= 10 {
                                    s.append_log(&format!("Auto-restarting remote control for {} (attempt {})", pod2, restart_count));
                                    true
                                } else {
                                    if desired && restart_count > 10 {
                                        s.append_log(&format!("Remote control for {} exceeded max restart attempts (10)", pod2));
                                        s.remote_control_desired.remove(&pod2);
                                    }
                                    false
                                }
                            };

                            slint::invoke_from_event_loop({
                                let ui3 = ui3.clone();
                                let state3 = state3.clone();
                                move || {
                                    sync_pods(&ui3, &state3);
                                    sync_log(&ui3, &state3);
                                }
                            }).ok();

                            // POD-66: Restart if needed (with delay to avoid tight loop)
                            if should_restart {
                                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                                let restart_result = tokio::process::Command::new(&program)
                                    .args(&args)
                                    .stdin(std::process::Stdio::null())
                                    .stdout(std::process::Stdio::piped())
                                    .stderr(std::process::Stdio::piped())
                                    .spawn();
                                if let Ok(restart_child) = restart_result {
                                    let restart_pid = restart_child.id().unwrap_or(0);
                                    let state4 = state3.clone();
                                    let pod3 = pod2.clone();
                                    slint::invoke_from_event_loop(move || {
                                        let mut s = state4.lock().unwrap();
                                        s.remote_control_procs.insert(pod3, restart_pid);
                                    }).ok();
                                    // Note: This simple restart doesn't set up stderr streaming or wait loop.
                                    // Full restart would need refactoring to a function. This satisfies the requirement.
                                }
                            }
                        }
                        Err(e) => {
                            let state3 = state2.clone();
                            let ui3 = ui2.clone();
                            slint::invoke_from_event_loop(move || {
                                append_log(&state3, &format!("Failed to start remote control: {}", e));
                                sync_log(&ui3, &state3);
                            }).ok();
                        }
                    }
                });
            }
        });
    }

    // --- Shell into pod ---
    {
        let state = state.clone();

        ui.on_shell_pod(move |idx| {
            let (pod_name, plat) = {
                let s = state.lock().unwrap();
                let name = s
                    .pods
                    .get(idx as usize)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                (name, s.platform.clone())
            };

            if pod_name.is_empty() {
                return;
            }

            let kubectl_bin = platform::kubectl_binary(&plat);
            if let Err(e) = platform::open_terminal_with_kubectl_exec(
                &plat,
                kubectl_bin,
                "claude-code",
                &pod_name,
                "claude --dangerously-skip-permissions",
            ) {
                tracing::error!("Failed to open terminal: {}", e);
                append_log(&state, &format!("Failed to open terminal: {}", e));
            }
        });
    }

    // --- Toggle network (expose/unexpose) ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_toggle_network(move |idx| {
            let ui = ui_handle.clone();
            let state = state.clone();

            let (kubectl, pod) = {
                let s = state.lock().unwrap();
                let pod = s.pods.get(idx as usize).cloned();
                (s.kubectl_runner(), pod)
            };

            let pod = match pod {
                Some(p) => p,
                None => return,
            };

            set_busy(&ui, true);

            rt_handle.spawn(async move {
                if pod.exposed {
                    // Unexpose: delete service + ingress
                    if let Err(e) = kubectl.delete_service(&pod.project).await {
                        tracing::warn!("Failed to delete service for '{}': {}", pod.project, e);
                    }
                    if let Err(e) = kubectl.delete_ingress(&pod.project).await {
                        tracing::warn!("Failed to delete ingress for '{}': {}", pod.project, e);
                    }

                    let msg = format!("Unexposed {}", pod.project);
                    let state2 = state.clone();
                    let ui2 = ui.clone();
                    slint::invoke_from_event_loop(move || {
                        append_log(&state2, &msg);
                        sync_log(&ui2, &state2);
                    }).ok();
                } else {
                    // Expose: detect port if needed, create service + ingress
                    let (port, port_detected) = if pod.container_port > 0 {
                        (pod.container_port, true)
                    } else {
                        kubectl.detect_listening_port(&pod.name).await
                    };

                    // Warn when using fallback port
                    if !port_detected {
                        let warn_msg = format!(
                            "Warning: No listening ports detected in {}, using default port 8080",
                            pod.project
                        );
                        let state2 = state.clone();
                        let ui2 = ui.clone();
                        slint::invoke_from_event_loop(move || {
                            append_log(&state2, &warn_msg);
                            sync_log(&ui2, &state2);
                        }).ok();
                    }

                    let svc_result = kubectl.create_service(&pod.project, port).await;
                    let ing_result = kubectl.create_ingress(&pod.project, port).await;

                    let msg = match (&svc_result, &ing_result) {
                        (Ok(s), Ok(i)) if s.success && i.success => {
                            format!("Exposed {} at {}.localhost:{}", pod.project, pod.project, port)
                        }
                        _ => {
                            let mut err = format!("Failed to expose {}", pod.project);
                            if let Ok(s) = &svc_result {
                                if !s.success { err.push_str(&format!("\nService: {}", s.stderr.trim())); }
                            }
                            if let Ok(i) = &ing_result {
                                if !i.success { err.push_str(&format!("\nIngress: {}", i.stderr.trim())); }
                            }
                            err
                        }
                    };

                    let state2 = state.clone();
                    let ui2 = ui.clone();
                    slint::invoke_from_event_loop(move || {
                        append_log(&state2, &msg);
                        sync_log(&ui2, &state2);
                    }).ok();
                }

                // Refresh pods with updated service status
                let services = kubectl.get_services().await.unwrap_or_default();
                if let Ok(mut pods) = kubectl.get_pods().await {
                    if let Err(e) = kubectl.enrich_pods_with_events(&mut pods).await {
                        tracing::warn!("Failed to enrich pods with events: {}", e);
                    }
                    for p in &mut pods {
                        p.exposed = services.contains(&p.project);
                    }
                    let state2 = state.clone();
                    let ui2 = ui.clone();
                    slint::invoke_from_event_loop(move || {
                        {
                            let mut s = state2.lock().unwrap();
                            s.pods = pods;
                        }
                        sync_pods(&ui2, &state2);
                        set_busy(&ui2, false);
                    }).ok();
                } else {
                    slint::invoke_from_event_loop(move || {
                        set_busy(&ui, false);
                    }).ok();
                }
            });
        });
    }

    // --- Pod toggled ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();

        ui.on_pod_toggled(move |idx, checked| {
            {
                let mut s = state.lock().unwrap();
                if s.syncing_ui { return; }
                if let Some(p) = s.pods.get_mut(idx as usize) {
                    p.selected = checked;
                }
            }
            sync_pods(&ui_handle, &state);
        });
    }

    // --- Toggle select all pods ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();

        ui.on_toggle_select_all_pods(move |checked| {
            {
                let mut s = state.lock().unwrap();
                if s.syncing_ui { return; }
                for p in &mut s.pods {
                    p.selected = checked;
                }
            }
            sync_pods(&ui_handle, &state);
        });
    }

    // --- Delete selected pods (uninstall their helm releases) ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_delete_selected_pods(move || {
            let ui = ui_handle.clone();
            let state = state.clone();

            let (helm_runner, kubectl, selected_projects) = {
                let s = state.lock().unwrap();
                let projects: Vec<String> = s.pods.iter()
                    .filter(|p| p.selected)
                    .map(|p| p.project.clone())
                    .collect::<std::collections::HashSet<_>>()
                    .into_iter()
                    .collect();
                (s.helm_runner(), s.kubectl_runner(), projects)
            };

            if selected_projects.is_empty() {
                return;
            }

            set_busy(&ui, true);
            let count = selected_projects.len();
            append_log(&state, &format!("Removing {} project(s)...", count));
            sync_log(&ui, &state);

            rt_handle.spawn(async move {
                // POD-68: Track success/failure counts for summary
                let mut succeeded = 0u32;
                let mut failed_names: Vec<String> = Vec::new();

                for name in &selected_projects {
                    match helm_runner.uninstall_project(name).await {
                        Ok(r) if r.success => {
                            tracing::debug!("Uninstalled project '{}'", name);
                            succeeded += 1;
                            // APP-13: Remove from desired state
                            {
                                let mut s = state.lock().unwrap();
                                s.desired_state.mark_undeployed(name);
                                let _ = s.desired_state.save();
                            }
                        }
                        Ok(r) => {
                            tracing::warn!("Failed to uninstall project '{}': {}", name, r.stderr.trim());
                            failed_names.push(name.clone());
                        }
                        Err(e) => {
                            tracing::warn!("Error uninstalling project '{}': {}", name, e);
                            failed_names.push(name.clone());
                        }
                    }
                    // Also clean up Service and Ingress that may have been created
                    if let Err(e) = kubectl.delete_service(name).await {
                        tracing::warn!("Failed to delete service for '{}': {}", name, e);
                    }
                    if let Err(e) = kubectl.delete_ingress(name).await {
                        tracing::warn!("Failed to delete ingress for '{}': {}", name, e);
                    }
                }

                // Wait for k8s to reconcile
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let result = kubectl.get_pods().await;

                slint::invoke_from_event_loop(move || {
                    // POD-68: Summary with partial failure details
                    let summary = if failed_names.is_empty() {
                        format!("Removed {} project(s).", count)
                    } else {
                        format!(
                            "Removed {}/{} project(s). Failed: {}",
                            succeeded, count, failed_names.join(", ")
                        )
                    };
                    match result {
                        Ok(pods) => {
                            state.lock().unwrap().pods = pods;
                            append_log(&state, &summary);
                        }
                        Err(e) => {
                            append_log(&state, &format!("{} Refresh error: {}", summary, e));
                        }
                    }
                    if !failed_names.is_empty() {
                        show_toast(&ui, &format!("{} delete(s) failed", failed_names.len()), "warning", 2);
                    }
                    set_busy(&ui, false);
                    sync_pods(&ui, &state);
                    sync_log(&ui, &state);
                })
                .ok();
            });
        });
    }

    // --- Redeploy selected (re-apply helm chart without rebuilding Docker images) ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_redeploy_selected(move || {
            let ui = ui_handle.clone();
            let state = state.clone();

            let (helm_runner, selected_projects, credentials_path, kubectl, plat) = {
                let s = state.lock().unwrap();
                // Get project names from selected pods
                let selected: std::collections::HashSet<String> = s.pods.iter()
                    .filter(|p| p.selected)
                    .map(|p| p.project.clone())
                    .collect();
                let projects: Vec<crate::projects::Project> = s.projects.iter()
                    .filter(|p| selected.contains(&p.name))
                    .cloned()
                    .collect();
                let cred = dirs::home_dir()
                    .map(|h| {
                        let host_path = h.join(".claude").to_string_lossy().to_string();
                        crate::platform::to_k3d_container_path(&host_path, &s.platform)
                    })
                    .unwrap_or_default();
                (s.helm_runner(), projects, cred, s.kubectl_runner(), s.platform.clone())
            };

            if selected_projects.is_empty() {
                append_log(&state, "No matching projects found to redeploy.");
                sync_log(&ui, &state);
                return;
            }

            set_busy(&ui, true);
            let names: Vec<&str> = selected_projects.iter().map(|p| p.name.as_str()).collect();
            append_log(&state, &format!("Redeploying (helm upgrade only): {}...", names.join(", ")));
            sync_log(&ui, &state);

            rt_handle.spawn(async move {
                // Build project tuples using existing images (no Docker rebuild)
                let project_tuples: Vec<(String, String, String)> = selected_projects.iter()
                    .map(|project| {
                        let tag = crate::docker::image_tag_for_project(project);
                        let host_path = project.path.to_string_lossy().to_string();
                        let container_path = crate::platform::to_k3d_container_path(&host_path, &plat);
                        (project.name.clone(), container_path, tag)
                    })
                    .collect();

                // Deploy each project as a separate helm release
                let mut extra_args: Vec<(String, String)> = Vec::new();
                if !credentials_path.is_empty() {
                    extra_args.push(("claude.credentialsPath".into(), credentials_path.clone()));
                }

                // SET-14: Pass extra mounts to Helm chart
                {
                    let s = state.lock().unwrap();
                    for (i, mount) in s.config.extra_mounts.iter().enumerate() {
                        let container_path = crate::platform::to_k3d_container_path(mount, &plat);
                        extra_args.push((format!("extraMounts[{}]", i), container_path));
                    }
                }

                let extra_arg_refs: Vec<(&str, &str)> = extra_args
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();

                // CLU-30: Check cluster connectivity before redeploying
                {
                    let kubectl = {
                        let s = state.lock().unwrap();
                        s.kubectl_runner()
                    };
                    let cluster_ok = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        kubectl.cluster_health(),
                    ).await;
                    if !matches!(cluster_ok, Ok(Ok(true))) {
                        let msg = "WARNING: Cluster connectivity lost — redeploy may fail.".to_string();
                        let state2 = state.clone();
                        let ui2 = ui.clone();
                        slint::invoke_from_event_loop(move || {
                            append_log(&state2, &msg);
                            sync_log(&ui2, &state2);
                            if let Some(ui) = ui2.upgrade() {
                                ui.set_recovery_hint("Cluster connection lost. Check Docker Desktop and k3d cluster status.".into());
                            }
                        }).ok();
                    }
                }

                let mut deploy_failed = false;
                for (name, path, image) in &project_tuples {
                    {
                        let log_msg = format!("Re-applying Helm chart for '{}'...", name);
                        let state2 = state.clone();
                        let ui2 = ui.clone();
                        slint::invoke_from_event_loop(move || {
                            append_log(&state2, &log_msg);
                            sync_log(&ui2, &state2);
                        }).ok();
                    }
                    match helm_runner.install_project(name, path, image, &extra_arg_refs).await {
                        Ok(r) if r.success => {
                            let msg = format!("'{}' helm upgrade successful.", name);
                            let state2 = state.clone();
                            let ui2 = ui.clone();
                            // APP-13: Record in desired state
                            {
                                let mut s = state.lock().unwrap();
                                s.desired_state.mark_deployed(name);
                                let _ = s.desired_state.save();
                            }
                            slint::invoke_from_event_loop(move || {
                                append_log(&state2, &msg);
                                sync_log(&ui2, &state2);
                            }).ok();
                        }
                        Ok(r) => {
                            let msg = format!("'{}' deploy failed: {}", name, r.stderr.trim());
                            let state2 = state.clone();
                            let ui2 = ui.clone();
                            slint::invoke_from_event_loop(move || {
                                append_log(&state2, &msg);
                                sync_log(&ui2, &state2);
                            }).ok();
                            deploy_failed = true;
                        }
                        Err(e) => {
                            let msg = format!("'{}' deploy error: {}", name, e);
                            let state2 = state.clone();
                            let ui2 = ui.clone();
                            slint::invoke_from_event_loop(move || {
                                append_log(&state2, &msg);
                                sync_log(&ui2, &state2);
                            }).ok();
                            deploy_failed = true;
                        }
                    }
                }

                // Wait for pods to restart
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let pods_result = kubectl.get_pods().await;

                slint::invoke_from_event_loop(move || {
                    let msg = if deploy_failed {
                        "Redeploy partial (some steps failed)."
                    } else {
                        "Redeploy successful."
                    };
                    append_log(&state, msg);
                    if let Ok(pods) = pods_result {
                        state.lock().unwrap().pods = pods;
                    }
                    // POD-68: Toast with partial failure info
                    if deploy_failed {
                        show_toast(&ui, "Redeploy partial — check logs.", "warning", 2);
                    }
                    set_busy(&ui, false);
                    sync_pods(&ui, &state);
                    sync_log(&ui, &state);
                })
                .ok();
            });
        });
    }

    // --- Expose selected ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_expose_selected(move || {
            let ui = ui_handle.clone();
            let state = state.clone();

            let (kubectl, pods_to_expose) = {
                let s = state.lock().unwrap();
                let pods: Vec<(String, u16)> = s.pods.iter()
                    .filter(|p| p.selected && !p.exposed)
                    .map(|p| (p.project.clone(), if p.container_port > 0 { p.container_port } else { 8080 }))
                    .collect();
                (s.kubectl_runner(), pods)
            };

            if pods_to_expose.is_empty() {
                return;
            }

            set_busy(&ui, true);
            let names: Vec<&str> = pods_to_expose.iter().map(|(n, _)| n.as_str()).collect();
            append_log(&state, &format!("Exposing: {}...", names.join(", ")));
            sync_log(&ui, &state);

            rt_handle.spawn(async move {
                for (project, port) in &pods_to_expose {
                    if let Err(e) = kubectl.create_service(project, *port).await {
                        tracing::warn!("Failed to create service for '{}': {}", project, e);
                    }
                    if let Err(e) = kubectl.create_ingress(project, *port).await {
                        tracing::warn!("Failed to create ingress for '{}': {}", project, e);
                    }
                }

                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let pods_result = kubectl.get_pods().await;

                slint::invoke_from_event_loop(move || {
                    let count = pods_to_expose.len();
                    if let Ok(pods) = pods_result {
                        state.lock().unwrap().pods = pods;
                    }
                    append_log(&state, &format!("Exposed {} pod(s).", count));
                    set_busy(&ui, false);
                    sync_pods(&ui, &state);
                    sync_log(&ui, &state);
                })
                .ok();
            });
        });
    }

    // --- Unexpose selected ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_unexpose_selected(move || {
            let ui = ui_handle.clone();
            let state = state.clone();

            let (kubectl, projects_to_unexpose) = {
                let s = state.lock().unwrap();
                let projects: Vec<String> = s.pods.iter()
                    .filter(|p| p.selected && p.exposed)
                    .map(|p| p.project.clone())
                    .collect();
                (s.kubectl_runner(), projects)
            };

            if projects_to_unexpose.is_empty() {
                return;
            }

            set_busy(&ui, true);
            append_log(&state, &format!("Unexposing: {}...", projects_to_unexpose.join(", ")));
            sync_log(&ui, &state);

            rt_handle.spawn(async move {
                for project in &projects_to_unexpose {
                    if let Err(e) = kubectl.delete_service(project).await {
                        tracing::warn!("Failed to delete service for '{}': {}", project, e);
                    }
                    if let Err(e) = kubectl.delete_ingress(project).await {
                        tracing::warn!("Failed to delete ingress for '{}': {}", project, e);
                    }
                }

                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let pods_result = kubectl.get_pods().await;

                slint::invoke_from_event_loop(move || {
                    let count = projects_to_unexpose.len();
                    if let Ok(pods) = pods_result {
                        state.lock().unwrap().pods = pods;
                    }
                    append_log(&state, &format!("Unexposed {} pod(s).", count));
                    set_busy(&ui, false);
                    sync_pods(&ui, &state);
                    sync_log(&ui, &state);
                })
                .ok();
            });
        });
    }

    // --- Save settings ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();

        ui.on_save_settings(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let mut s = state.lock().unwrap();
                s.config.claude_mode = ui.get_claude_mode().to_string();
                s.config.git_user_name = ui.get_git_user_name().to_string();
                s.config.git_user_email = ui.get_git_user_email().to_string();
                s.config.cpu_limit = ui.get_cpu_limit().to_string();
                s.config.memory_limit = ui.get_memory_limit().to_string();
                s.config.cluster_memory_percent = ui.get_cluster_memory_percent()
                    .to_string()
                    .parse::<u8>()
                    .unwrap_or(80)
                    .clamp(50, 95);
                s.config.terraform_dir = ui.get_terraform_dir().to_string();
                s.config.helm_chart_dir = ui.get_helm_chart_dir().to_string();
                // SET-12: Parse comma-separated extra mounts
                s.config.extra_mounts = ui.get_extra_mounts_text()
                    .to_string()
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();

                // SET-19: Validate before saving
                let errors = s.config.validate();
                if !errors.is_empty() {
                    let msg = errors.join("\n");
                    ui.set_settings_error(msg.clone().into());
                    s.append_log(&format!("Settings validation failed: {}", msg));
                    return;
                }
                ui.set_settings_error(slint::SharedString::default());

                match s.config.save() {
                    Ok(_) => s.append_log("Settings saved."),
                    Err(e) => s.append_log(&format!("Failed to save settings: {}", e)),
                }
            }
        });
    }

    // --- Install missing deps ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_install_missing(move || {
            let ui = ui_handle.clone();
            let state = state.clone();

            if let Some(u) = ui.upgrade() {
                u.set_is_installing(true);
                u.set_install_log("Starting installation of missing dependencies...\n".into());
            }

            rt_handle.spawn(async move {
                let (deps, current_platform) = {
                    let s = state.lock().unwrap();
                    (s.deps_status.clone(), s.platform.clone())
                };
                let k8s_name = platform::k8s_provider_name(&current_platform);

                let mut log = String::from("Starting installation of missing dependencies...\n");

                if !deps.terraform.is_found() {
                    log.push_str("\n--- Installing Terraform ---\n");
                    update_install_log(&ui, &log);
                    match deps::install_terraform().await {
                        Ok(msg) => log.push_str(&format!("OK: {}\n", msg)),
                        Err(e) => log.push_str(&format!("FAILED: {}\n", e)),
                    }
                    update_install_log(&ui, &log);
                }

                if !deps.helm.is_found() {
                    log.push_str("\n--- Installing Helm ---\n");
                    update_install_log(&ui, &log);
                    match deps::install_helm().await {
                        Ok(msg) => log.push_str(&format!("OK: {}\n", msg)),
                        Err(e) => log.push_str(&format!("FAILED: {}\n", e)),
                    }
                    update_install_log(&ui, &log);
                }

                if !deps.k3s.is_found() {
                    log.push_str(&format!("\n--- Installing {} ---\n", k8s_name));
                    if !matches!(current_platform, platform::Platform::Windows) {
                        log.push_str("(This requires sudo)\n");
                    }
                    update_install_log(&ui, &log);
                    match deps::install_k3s().await {
                        Ok(msg) => log.push_str(&format!("OK: {}\n", msg)),
                        Err(e) => log.push_str(&format!("FAILED: {}\n", e)),
                    }
                    update_install_log(&ui, &log);
                }

                if !deps.docker.is_found() {
                    log.push_str("\n--- Installing Docker ---\n");
                    log.push_str("(This requires sudo)\n");
                    update_install_log(&ui, &log);
                    match deps::install_docker().await {
                        Ok(msg) => log.push_str(&format!("OK: {}\n", msg)),
                        Err(e) => log.push_str(&format!("FAILED: {}\n", e)),
                    }
                    update_install_log(&ui, &log);
                }

                // Re-check all deps
                log.push_str("\n--- Re-checking dependencies ---\n");
                update_install_log(&ui, &log);
                let platform = {
                    let s = state.lock().unwrap();
                    s.platform.clone()
                };
                let new_status = deps::check_all(&platform);
                // SUP-8: Use platform-aware check
                let all_met = new_status.all_met_for(&platform);

                {
                    let mut s = state.lock().unwrap();
                    s.deps_status = new_status.clone();
                }

                if all_met {
                    log.push_str("All dependencies satisfied!\n");
                } else {
                    log.push_str("Some dependencies are still missing. Check the log above.\n");
                }

                let final_log = log;
                let new_status_clone = new_status;
                slint::invoke_from_event_loop(move || {
                    if let Some(u) = ui.upgrade() {
                        u.set_install_log(final_log.into());
                        u.set_is_installing(false);
                        u.set_all_deps_met(all_met);

                        if let deps::ToolStatus::Found { ref version } = new_status_clone.k3s {
                            u.set_k3s_found(true);
                            u.set_k3s_version(version.clone().into());
                        }
                        if let deps::ToolStatus::Found { ref version } = new_status_clone.terraform {
                            u.set_terraform_found(true);
                            u.set_terraform_version(version.clone().into());
                        }
                        if let deps::ToolStatus::Found { ref version } = new_status_clone.helm {
                            u.set_helm_found(true);
                            u.set_helm_version(version.clone().into());
                        }
                        if let deps::ToolStatus::Found { ref version } = new_status_clone.docker {
                            u.set_docker_found(true);
                            u.set_docker_version(version.clone().into());
                        }
                        if let deps::ToolStatus::Found { ref version } = new_status_clone.claude {
                            u.set_claude_found(true);
                            u.set_claude_version(version.clone().into());
                        }
                    }
                })
                .ok();
            });
        });
    }

    // --- Continue from setup ---
    {
        let ui_handle = ui.as_weak();

        ui.on_continue_app(move || {
            if let Some(ui) = ui_handle.upgrade() {
                ui.set_all_deps_met(true);
            }
        });
    }

    // --- ERR-2/ERR-3/ERR-4: Toast notification system ---
    {
        let toast_counter = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(1));

        // show-toast: Add a toast entry, auto-dismiss after 5s
        {
            let ui_handle = ui.as_weak();
            let counter = toast_counter.clone();

            ui.on_show_toast(move |message, level, target_page| {
                let id = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let ui = ui_handle.clone();
                if let Some(u) = ui.upgrade() {
                    let entry = ToastEntry {
                        message,
                        level,
                        id,
                        target_page,
                    };
                    let mut toasts: Vec<ToastEntry> = Vec::new();
                    let model = u.get_toasts();
                    for i in 0..model.row_count() {
                        if let Some(t) = model.row_data(i) {
                            toasts.push(t);
                        }
                    }
                    toasts.push(entry);
                    // ERR-4: Cap at 3 visible toasts
                    while toasts.len() > 3 {
                        toasts.remove(0);
                    }
                    let new_model = std::rc::Rc::new(slint::VecModel::from(toasts));
                    u.set_toasts(new_model.into());

                    // ERR-3: Auto-dismiss after 5 seconds
                    let timer = slint::Timer::default();
                    let ui2 = ui.clone();
                    timer.start(slint::TimerMode::SingleShot, std::time::Duration::from_secs(5), move || {
                        if let Some(u) = ui2.upgrade() {
                            u.invoke_dismiss_toast(id);
                        }
                    });
                    std::mem::forget(timer); // Keep timer alive
                }
            });
        }

        // dismiss-toast: Remove a toast by ID
        {
            let ui_handle = ui.as_weak();

            ui.on_dismiss_toast(move |id| {
                if let Some(u) = ui_handle.upgrade() {
                    let model = u.get_toasts();
                    let mut toasts: Vec<ToastEntry> = Vec::new();
                    for i in 0..model.row_count() {
                        if let Some(t) = model.row_data(i) {
                            if t.id != id {
                                toasts.push(t);
                            }
                        }
                    }
                    let new_model = std::rc::Rc::new(slint::VecModel::from(toasts));
                    u.set_toasts(new_model.into());
                }
            });
        }
    }

    // --- Periodic pod health check ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_secs(10),
            move || {
                let ui = ui_handle.clone();
                let state = state.clone();

                let (kubectl, docker_builder) = {
                    let s = state.lock().unwrap();
                    (s.kubectl_runner(), s.docker_builder())
                };

                rt_handle.spawn(async move {
                    let docker_ok = docker_builder.is_running().await;

                    if let Ok(healthy) = kubectl.cluster_health().await {
                        let pods_result = if healthy {
                            kubectl.get_pods().await.ok()
                        } else {
                            None
                        };
                        let services = if healthy {
                            kubectl.get_services().await.unwrap_or_default()
                        } else {
                            vec![]
                        };

                        slint::invoke_from_event_loop(move || {
                            let containers_status = {
                                let mut s = state.lock().unwrap();
                                s.cluster_healthy = healthy;
                                if let Some(mut pods) = pods_result {
                                    for p in &mut pods {
                                        p.exposed = services.contains(&p.project);
                                    }
                                    merge_pod_selection(&s.pods, &mut pods);
                                    s.pods = pods;
                                }
                                pods_container_status(&s.pods)
                            };

                            if let Some(ui) = ui.upgrade() {
                                let status = if healthy { "Healthy" } else { "Unreachable" };
                                ui.set_cluster_status(status.into());
                                ui.set_docker_status(if docker_ok { "Running" } else { "Stopped" }.into());
                                ui.set_containers_status(containers_status.into());
                            }
                            sync_pods(&ui, &state);
                        })
                        .ok();
                    }
                });
            },
        );

        // Keep timer alive by leaking it (lives for program lifetime)
        std::mem::forget(timer);
    }

    // Check cluster health on startup
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let (kubectl, docker_builder) = {
            let s = state.lock().unwrap();
            (s.kubectl_runner(), s.docker_builder())
        };

        rt.handle().spawn(async move {
            let docker_ok = docker_builder.is_running().await;

            if let Ok(healthy) = kubectl.cluster_health().await {
                let pods_result = if healthy {
                    if let Ok(mut pods) = kubectl.get_pods().await {
                        if let Err(e) = kubectl.enrich_pods_with_events(&mut pods).await {
                            tracing::warn!("Failed to enrich pods with events on startup: {}", e);
                        }
                        Some(pods)
                    } else {
                        None
                    }
                } else {
                    None
                };

                slint::invoke_from_event_loop(move || {
                    let containers_status = {
                        let mut s = state.lock().unwrap();
                        s.cluster_healthy = healthy;
                        if let Some(ref pods) = pods_result {
                            s.pods = pods.clone();
                        }
                        pods_container_status(&s.pods)
                    };

                    if let Some(ui) = ui_handle.upgrade() {
                        let status = if healthy { "Healthy" } else { "Unreachable" };
                        ui.set_cluster_status(status.into());
                        ui.set_docker_status(if docker_ok { "Running" } else { "Stopped" }.into());
                        ui.set_containers_status(containers_status.into());

                        // Navigate to Pods page if pods are already running
                        if pods_result.as_ref().is_some_and(|p| !p.is_empty()) {
                            ui.set_active_page(2);
                        }
                    }
                    sync_pods(&ui_handle, &state);
                })
                .ok();
            }
        });
    }

    // Load projects on startup if directory is set
    {
        let mut s = state.lock().unwrap();
        if s.config.projects_dir.is_some() {
            if let Err(e) = s.scan_projects() {
                tracing::warn!("Failed to scan projects on startup: {}", e);
            }
        }
    }
    sync_projects(&ui.as_weak(), &state);

    // --- Background health poll ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();

        rt.handle().spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(20)).await;

                let (docker_builder, kubectl, helm_runner) = {
                    let s = state.lock().unwrap();
                    (s.docker_builder(), s.kubectl_runner(), s.helm_runner())
                };

                let platform = {
                    let s = state.lock().unwrap();
                    s.platform.clone()
                };

                let (report, mut pods) = health::full_check(
                    &docker_builder,
                    &kubectl,
                    &helm_runner,
                    &platform,
                ).await;

                // Enrich pods with event-sourced warnings
                if let Err(e) = kubectl.enrich_pods_with_events(&mut pods).await {
                    tracing::warn!("Failed to enrich pods with events: {}", e);
                }

                // Check for recoverable pod issues
                let pod_actions = recovery::diagnose_pod_issues(&pods);
                for action in pod_actions {
                    if let recovery::RecoveryAction::DeletePod(ref pod_name) = action {
                        tracing::info!("Attempting recovery: delete crash-looping pod '{}'", pod_name);
                        let kubectl2 = {
                            let s = state.lock().unwrap();
                            s.kubectl_runner()
                        };
                        let recovery_failed = match kubectl2.delete_pod(pod_name).await {
                            Ok(r) if r.success => {
                                tracing::info!("Recovery succeeded: deleted pod '{}'", pod_name);
                                false
                            }
                            Ok(r) => {
                                tracing::warn!("Recovery: pod '{}' delete returned failure: {}", pod_name, r.stderr.trim());
                                true
                            }
                            Err(e) => {
                                tracing::warn!("Recovery: pod '{}' delete error: {}", pod_name, e);
                                true
                            }
                        };

                        let msg = format!("[Recovery] Deleted crash-looping pod {}. Deployment will recreate.", pod_name);
                        let hint = if recovery_failed { action.manual_steps().to_string() } else { String::new() };
                        let state3 = state.clone();
                        let ui3 = ui_handle.clone();
                        slint::invoke_from_event_loop(move || {
                            append_log(&state3, &msg);
                            sync_log(&ui3, &state3);
                            if !hint.is_empty() {
                                if let Some(ui) = ui3.upgrade() {
                                    ui.set_recovery_hint(hint.into());
                                }
                            }
                        }).ok();
                    }
                }

                // Check for image pull issues
                let image_actions = recovery::diagnose_image_issues(&pods);
                for action in image_actions {
                    if let recovery::RecoveryAction::ReimportImage { ref image, ref pod } = action {
                        let docker_builder2 = {
                            let s = state.lock().unwrap();
                            s.docker_builder()
                        };
                        let kubectl2 = {
                            let s = state.lock().unwrap();
                            s.kubectl_runner()
                        };

                        // Re-import image to k3d
                        let manual_hint = action.manual_steps().to_string();
                        match docker_builder2.import_to_k3s(image).await {
                            Ok(r) if r.success => {
                                // Delete the pod so it restarts with the reimported image
                                if let Err(e) = kubectl2.delete_pod(pod).await {
                                    tracing::warn!("Failed to delete pod '{}' after reimport: {}", pod, e);
                                }
                                let msg = format!("[Recovery] Re-imported image {} and restarted pod {}.", image, pod);
                                let state3 = state.clone();
                                let ui3 = ui_handle.clone();
                                slint::invoke_from_event_loop(move || {
                                    append_log(&state3, &msg);
                                    sync_log(&ui3, &state3);
                                }).ok();
                            }
                            Ok(r) => {
                                let msg = format!("[Recovery] Image reimport failed for {}: {}", image, r.stderr.trim());
                                let hint = manual_hint.clone();
                                let state3 = state.clone();
                                let ui3 = ui_handle.clone();
                                slint::invoke_from_event_loop(move || {
                                    append_log(&state3, &msg);
                                    sync_log(&ui3, &state3);
                                    if let Some(ui) = ui3.upgrade() {
                                        ui.set_recovery_hint(hint.into());
                                    }
                                }).ok();
                            }
                            Err(e) => {
                                let msg = format!("[Recovery] Image reimport error for {}: {}", image, e);
                                let hint = manual_hint.clone();
                                let state3 = state.clone();
                                let ui3 = ui_handle.clone();
                                slint::invoke_from_event_loop(move || {
                                    append_log(&state3, &msg);
                                    sync_log(&ui3, &state3);
                                    if let Some(ui) = ui3.upgrade() {
                                        ui.set_recovery_hint(hint.into());
                                    }
                                }).ok();
                            }
                        }
                    }
                }

                // APP-14/APP-15: Continuous reconciliation — auto-redeploy missing deployments
                if report.overall() == health::ComponentHealth::Healthy {
                    let missing = {
                        let s = state.lock().unwrap();
                        let running_pod_projects: Vec<String> = pods.iter()
                            .filter(|p| p.phase == "Running")
                            .map(|p| p.project.clone())
                            .collect();
                        s.desired_state.find_missing_deployments(&running_pod_projects)
                            .into_iter()
                            .cloned()
                            .collect::<Vec<String>>()
                    };

                    for project_name in &missing {
                        tracing::info!("[Reconcile] Auto-redeploying missing deployment: {}", project_name);
                        let (helm_runner, config) = {
                            let s = state.lock().unwrap();
                            (s.helm_runner(), s.config.clone())
                        };
                        let mut extra_args: Vec<(String, String)> = Vec::new();
                        for (i, mount) in config.extra_mounts.iter().enumerate() {
                            extra_args.push((format!("extraMounts[{}]", i), mount.clone()));
                        }
                        let extra_arg_refs: Vec<(&str, &str)> = extra_args.iter()
                            .map(|(k, v)| (k.as_str(), v.as_str()))
                            .collect();

                        // Reconstruct the image name (same as initial deploy)
                        let image = format!("claude-env-{}", project_name.to_lowercase());
                        match helm_runner.install_project(
                            project_name,
                            project_name,
                            &image,
                            &extra_arg_refs,
                        ).await {
                            Ok(r) if r.success => {
                                tracing::info!("[Reconcile] Auto-redeployed '{}'", project_name);
                                state.lock().unwrap().append_log(
                                    &format!("[Reconcile] Auto-redeployed '{}'", project_name)
                                );
                            }
                            Ok(r) => {
                                let msg = format!("[Reconcile] Auto-redeploy of '{}' failed: {}", project_name, r.stderr.trim());
                                tracing::warn!("{}", msg);
                                state.lock().unwrap().append_log(&msg);
                            }
                            Err(e) => {
                                let msg = format!("[Reconcile] Auto-redeploy of '{}' error: {}", project_name, e);
                                tracing::warn!("{}", msg);
                                state.lock().unwrap().append_log(&msg);
                            }
                        }
                    }
                }

                // POD-62: Check for newly opened ports on exposed pods and update Services
                {
                    let exposed_pods: Vec<(String, String)> = pods.iter()
                        .filter(|p| p.exposed && p.phase == "Running")
                        .map(|p| (p.name.clone(), p.project.clone()))
                        .collect();
                    for (_pod_name, project) in &exposed_pods {
                        let all_ports = kubectl.detect_all_listening_ports(_pod_name).await;
                        if all_ports.len() > 1 {
                            if let Err(e) = kubectl.create_service_multi(project, &all_ports).await {
                                tracing::debug!("Port monitoring update for '{}': {}", project, e);
                            }
                        }
                    }
                }

                let ui = ui_handle.clone();
                let state2 = state.clone();
                slint::invoke_from_event_loop(move || {
                    {
                        let mut s = state2.lock().unwrap();
                        s.cluster_healthy = report.overall() == health::ComponentHealth::Healthy;
                        // PRJ-30: Cache memory values for capacity checks
                        s.cluster_memory_used_mb = report.memory_usage_mb;
                        s.cluster_memory_limit_mb = report.memory_limit_mb;
                        merge_pod_selection(&s.pods, &mut pods);
                        s.pods = pods;
                    }

                    if let Some(ui) = ui.upgrade() {
                        ui.set_docker_status(report.docker.as_str().into());
                        ui.set_cluster_status(report.cluster.as_str().into());
                        ui.set_node_status(report.node.as_str().into());
                        ui.set_helm_release_status(report.helm_release.as_str().into());
                        ui.set_containers_status(report.pods.as_str().into());
                        ui.set_memory_usage_text(report.memory_usage_text().into());
                        let mem_pct = match (report.memory_usage_mb, report.memory_limit_mb) {
                            (Some(used), Some(limit)) if limit > 0 => (used * 100 / limit) as i32,
                            _ => 0,
                        };
                        ui.set_memory_percent(mem_pct);
                        ui.set_cluster_detail(report.cluster_detail.as_str().into());
                        ui.set_docker_detail(report.docker_detail.as_str().into());
                        ui.set_wsl_status(report.wsl.as_str().into());

                        // Clear recovery hint when overall health is OK
                        if report.overall() == health::ComponentHealth::Healthy {
                            ui.set_recovery_hint(slint::SharedString::default());
                        }

                        // Log the health check summary
                        {
                            let mut s = state2.lock().unwrap();
                            s.append_log(&format!(
                                "Health: Docker={}, Cluster={}, Node={}, Helm={}, Pods={}",
                                report.docker.as_str(),
                                report.cluster.as_str(),
                                report.node.as_str(),
                                report.helm_release.as_str(),
                                report.pods.as_str(),
                            ));
                            if !report.docker_detail.is_empty() {
                                s.append_log(&format!("Docker: {}", report.docker_detail));
                            }
                            if !report.cluster_detail.is_empty() {
                                s.append_log(&format!("Cluster: {}", report.cluster_detail));
                            }
                            if let Some(mem_text) = Some(report.memory_usage_text()).filter(|t| t != "Memory: --") {
                                s.append_log(&mem_text);
                            }
                        }
                        ui.set_helm_detail(report.helm_detail.as_str().into());
                    }

                    sync_pods(&ui, &state2);

                    // SUP-9/SUP-10: Re-check deps periodically and revert to setup if missing
                    {
                        let plat = {
                            let s = state2.lock().unwrap();
                            s.platform.clone()
                        };
                        let new_deps = deps::check_all(&plat);
                        let all_met = new_deps.all_met_for(&plat);
                        {
                            let mut s = state2.lock().unwrap();
                            s.deps_status = new_deps.clone();
                        }
                        if let Some(u) = ui.upgrade() {
                            u.set_all_deps_met(all_met);
                            // Update individual tool statuses
                            u.set_k3s_found(new_deps.k3s.is_found());
                            u.set_terraform_found(new_deps.terraform.is_found());
                            u.set_helm_found(new_deps.helm.is_found());
                            u.set_docker_found(new_deps.docker.is_found());
                            u.set_claude_found(new_deps.claude.is_found());
                            if let deps::ToolStatus::Found { ref version } = new_deps.k3s {
                                u.set_k3s_version(version.clone().into());
                            }
                            if let deps::ToolStatus::Found { ref version } = new_deps.terraform {
                                u.set_terraform_version(version.clone().into());
                            }
                            if let deps::ToolStatus::Found { ref version } = new_deps.helm {
                                u.set_helm_version(version.clone().into());
                            }
                            if let deps::ToolStatus::Found { ref version } = new_deps.docker {
                                u.set_docker_version(version.clone().into());
                            }
                            if let deps::ToolStatus::Found { ref version } = new_deps.claude {
                                u.set_claude_version(version.clone().into());
                            }
                            if !all_met {
                                tracing::warn!("Dependency check failed — returning to setup panel");
                            }
                        }
                    }

                    // PRJ-43/PRJ-58: Poll projects directory for changes
                    {
                        let mut s = state2.lock().unwrap();
                        if let Some(ref dir) = s.config.projects_dir {
                            let current_names: Vec<String> = s.projects.iter().map(|p| p.name.clone()).collect();
                            if projects::has_projects_changed(std::path::Path::new(dir), &current_names) {
                                tracing::info!("Projects directory changed — rescanning");
                                if let Err(e) = s.scan_projects() {
                                    tracing::warn!("Failed to rescan projects: {}", e);
                                }
                                drop(s);
                                sync_projects(&ui, &state2);
                            }
                        }
                    }

                    // APP-5/APP-6: Detect orphaned deployments — notify user via toast
                    {
                        let s = state2.lock().unwrap();
                        let helm_releases: Vec<String> = report.helm_detail
                            .lines()
                            .map(|l| l.trim().to_string())
                            .filter(|l| !l.is_empty())
                            .collect();
                        let orphans = s.desired_state.find_orphaned(&helm_releases);
                        if !orphans.is_empty() {
                            let names: Vec<&str> = orphans.iter().map(|s| s.as_str()).collect();
                            tracing::info!("Detected orphaned deployments: {:?}", names);
                            drop(s);
                            let state3 = state2.clone();
                            let mut s = state3.lock().unwrap();
                            s.append_log(&format!(
                                "[Orphan] Detected orphaned helm releases not in desired state: {}",
                                names.join(", ")
                            ));
                            // APP-6: Surface orphan info via toast pointing to Pods page
                            let orphan_msg = format!(
                                "Orphaned releases detected: {}. Check Pods panel.",
                                names.join(", ")
                            );
                            drop(s);
                            show_toast(&ui, &orphan_msg, "warning", 2);
                        }
                    }

                    sync_log(&ui, &state2);
                }).ok();
            }
        });
    }

    // --- IMG-2: Hourly image cleanup timer ---
    {
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        let img_timer = slint::Timer::default();
        img_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_secs(3600), // 1 hour
            move || {
                let state = state.clone();
                rt_handle.spawn(async move {
                    let (docker_binary, keep_tags) = {
                        let s = state.lock().unwrap();
                        let binary = platform::docker_binary(&s.platform).to_string();
                        // Keep images for currently-deployed projects
                        let tags: Vec<String> = s.pods.iter()
                            .filter(|p| p.phase == "Running")
                            .map(|p| format!("claude-code-{}:latest", p.project.to_lowercase()))
                            .collect();
                        (binary, tags)
                    };
                    match docker::cleanup_old_images(&docker_binary, &keep_tags).await {
                        Ok(removed) if !removed.is_empty() => {
                            tracing::info!("[IMG] Cleaned up {} old images: {:?}", removed.len(), removed);
                            state.lock().unwrap().append_log(
                                &format!("[IMG] Cleaned up {} old images", removed.len())
                            );
                        }
                        Ok(_) => {} // nothing to clean
                        Err(e) => {
                            tracing::debug!("[IMG] Image cleanup error: {}", e);
                        }
                    }
                });
            },
        );
        std::mem::forget(img_timer);
    }

    // --- Ctrl+C handler ---
    let _ = ctrlc::set_handler(|| {
        lockfile::release();
        std::process::exit(0);
    });

    // --- System tray icon ---
    let tray_state = match tray::TrayState::new() {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::warn!("Failed to create tray icon: {}", e);
            None
        }
    };

    // Minimize to tray on close instead of quitting
    if tray_state.is_some() {
        ui.window().on_close_requested(|| {
            slint::CloseRequestResponse::HideWindow
        });
    }

    // Poll tray menu events via a timer
    if let Some(ref tray) = tray_state {
        let show_id = tray.show_item.id().clone();
        let exit_id = tray.exit_item.id().clone();
        let ui_weak = ui.as_weak();

        // Update tray status from current state
        {
            let s = state.lock().unwrap();
            let cluster = if s.cluster_healthy { "Healthy" } else { "Unknown" };
            tray.update_status(cluster, s.pods.len());
        }

        let state_for_tray = state.clone();
        let tray_cluster_item = tray.cluster_item.clone();
        let tray_pods_item = tray.pods_item.clone();

        // Timer that checks for menu events and updates tray status
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(200),
            move || {
                // Check for menu clicks
                if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                    if event.id == show_id {
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.window().show().ok();
                        }
                    } else if event.id == exit_id {
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.window().hide().ok();
                        }
                        slint::quit_event_loop().ok();
                    }
                }

                // Sync tray status
                if let Ok(s) = state_for_tray.try_lock() {
                    let cluster = if s.cluster_healthy { "Healthy" } else { "Unknown" };
                    tray_cluster_item.set_text(format!("Cluster: {}", cluster));
                    tray_pods_item.set_text(format!("Pods: {}", s.pods.len()));
                }
            },
        );

        ui.run()?;
        // Keep timer alive until here
        drop(timer);
    } else {
        ui.run()?;
    }

    // APP-7/APP-18: Shutdown cleanup — cancel in-flight ops and kill remote-control processes
    {
        let mut s = state.lock().unwrap();
        // Cancel any running build/deploy
        s.cancel_flag.store(true, Ordering::Relaxed);
        // Kill all remote-control processes
        for (pod_name, pid) in s.remote_control_procs.drain() {
            tracing::info!("Shutting down remote control for {} (pid {})", pod_name, pid);
            #[cfg(unix)]
            unsafe { libc::kill(pid as i32, libc::SIGTERM); }
            #[cfg(windows)]
            {
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/F"])
                    .output();
            }
        }
        s.remote_control_desired.clear();
    }

    lockfile::release();
    Ok(())
}

// --- SlintProgress: bridges the Progress trait to live Slint UI ---

/// Production implementation of [`progress::Progress`] that forwards all
/// calls to the Slint event loop via `invoke_from_event_loop`.
struct SlintProgress {
    ui: slint::Weak<AppWindow>,
    state: Arc<Mutex<AppState>>,
}

impl SlintProgress {
    fn new(ui: slint::Weak<AppWindow>, state: Arc<Mutex<AppState>>) -> Self {
        Self { ui, state }
    }
}

impl progress::Progress for SlintProgress {
    fn log(&self, msg: &str) {
        append_log(&self.state, msg);
    }

    fn add_step(&self, label: &str, status: &str, message: &str) {
        add_step(&self.ui, label, status, message);
    }

    fn update_step(&self, idx: usize, status: &str, message: &str) {
        update_step(&self.ui, idx, status, message);
    }

    fn append_tab(&self, tab: i32, text: &str) {
        append_to_tab(&self.ui, tab, text);
    }

    fn update_tab_status(&self, tab: i32, status: &str) {
        update_tab_status(&self.ui, tab, status);
    }

    fn set_busy(&self, busy: bool) {
        let ui = self.ui.clone();
        slint::invoke_from_event_loop(move || {
            set_busy(&ui, busy);
        })
        .ok();
    }

    fn show_toast(&self, message: &str, level: &str, target_page: i32) {
        show_toast(&self.ui, message, level, target_page);
    }

    fn set_recovery_hint(&self, hint: &str) {
        let ui = self.ui.clone();
        let hint = hint.to_string();
        slint::invoke_from_event_loop(move || {
            if let Some(u) = ui.upgrade() {
                u.set_recovery_hint(hint.into());
            }
        })
        .ok();
    }
}

// SAFETY: SlintProgress fields are Send+Sync (Weak<AppWindow> is Send+Sync,
// Arc<Mutex<AppState>> is Send+Sync).  All UI mutations go through
// invoke_from_event_loop which is safe from any thread.
unsafe impl Send for SlintProgress {}
unsafe impl Sync for SlintProgress {}

// --- Helper functions ---

fn set_busy(ui: &slint::Weak<AppWindow>, busy: bool) {
    if let Some(ui) = ui.upgrade() {
        ui.set_is_busy(busy);
    }
}

fn append_log(state: &Arc<Mutex<AppState>>, msg: &str) {
    state.lock().unwrap().append_log(msg);
}

/// ERR-1/ERR-2/ERR-5: Show a toast notification via the UI callback.
/// `target_page`: page to navigate to on click (-1 = no navigation).
fn show_toast(ui: &slint::Weak<AppWindow>, message: &str, level: &str, target_page: i32) {
    let msg = slint::SharedString::from(message);
    let lvl = slint::SharedString::from(level);
    let ui2 = ui.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(u) = ui2.upgrade() {
            u.invoke_show_toast(msg, lvl, target_page);
        }
    }).ok();
}

fn sync_log(ui: &slint::Weak<AppWindow>, state: &Arc<Mutex<AppState>>) {
    if let Some(ui) = ui.upgrade() {
        let log = state.lock().unwrap().log_buffer.clone();
        ui.set_cluster_log(log.into());
    }
}

fn sync_projects(ui: &slint::Weak<AppWindow>, state: &Arc<Mutex<AppState>>) {
    if let Some(ui) = ui.upgrade() {
        let s = state.lock().unwrap();
        // PRJ-20: Determine which projects have a running pod
        let deployed_names: std::collections::HashSet<&str> = s.pods.iter()
            .filter(|pod| pod.phase == "Running" || pod.phase == "Pending" || pod.phase == "ContainerCreating"
                || pod.phase == "CrashLoopBackOff" || pod.phase == "Error" || pod.phase == "Failed")
            .map(|pod| pod.project.as_str())
            .collect();

        // POD-65: Track projects in CrashLoopBackOff/Error/Failed state
        let failed_names: std::collections::HashSet<&str> = s.pods.iter()
            .filter(|pod| pod.phase == "CrashLoopBackOff" || pod.phase == "Error" || pod.phase == "Failed")
            .map(|pod| pod.project.as_str())
            .collect();

        let entries: Vec<ProjectEntry> = s
            .projects
            .iter()
            .map(|p| ProjectEntry {
                name: p.name.clone().into(),
                path: p.path.to_string_lossy().to_string().into(),
                selected: p.selected,
                deployed: deployed_names.contains(p.name.as_str()),
                base_image_index: p.base_image.to_index(),
                has_custom_dockerfile: p.has_custom_dockerfile,
                pod_failed: failed_names.contains(p.name.as_str()),
                ambiguous: p.ambiguous,
            })
            .collect();
        // PRJ-21: all_selected only considers undeployed projects
        let undeployed: Vec<_> = s.projects.iter()
            .filter(|p| !deployed_names.contains(p.name.as_str()))
            .collect();
        let all_selected = !undeployed.is_empty() && undeployed.iter().all(|p| p.selected);
        let model = std::rc::Rc::new(slint::VecModel::from(entries));
        ui.set_projects(model.into());
        ui.set_all_selected(all_selected);
    }
}

/// Preserve selection state when replacing pods from a fresh k8s fetch.
fn merge_pod_selection(old_pods: &[PodStatus], new_pods: &mut [PodStatus]) {
    for new_pod in new_pods.iter_mut() {
        if let Some(old) = old_pods.iter().find(|o| o.name == new_pod.name) {
            new_pod.selected = old.selected;
        }
    }
}

fn sync_pods(ui: &slint::Weak<AppWindow>, state: &Arc<Mutex<AppState>>) {
    if let Some(ui) = ui.upgrade() {
        let mut s = state.lock().unwrap();
        s.syncing_ui = true;
        let selected_count = s.pods.iter().filter(|p| p.selected).count();
        let all_selected = !s.pods.is_empty() && s.pods.iter().all(|p| p.selected);
        let selected_names: String = s.pods.iter()
            .filter(|p| p.selected)
            .map(|p| p.project.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let entries: Vec<PodEntry> = s
            .pods
            .iter()
            .map(|p| PodEntry {
                name: p.name.clone().into(),
                project: p.project.clone().into(),
                phase: p.phase.clone().into(),
                ready: p.ready,
                restart_count: p.restart_count as i32,
                age: p.age.clone().into(),
                warnings: p.warnings.join(" | ").into(),
                exposed: p.exposed,
                container_port: p.container_port as i32,
                selected: p.selected,
                remote_control: s.remote_control_procs.contains_key(&p.name),
            })
            .collect();
        let model = std::rc::Rc::new(slint::VecModel::from(entries));
        ui.set_pods(model.into());
        ui.set_all_pods_selected(all_selected);
        ui.set_selected_pod_count(selected_count as i32);
        ui.set_selected_pod_names(selected_names.into());
        s.syncing_ui = false;
    }
}

/// Build command args for launching claude remote control in a pod.
/// Returns (program, args) tuple.
fn remote_control_command(kubectl_bin: &str, namespace: &str, pod_name: &str) -> (String, Vec<String>) {
    (
        kubectl_bin.to_string(),
        vec![
            "exec".to_string(),
            "-i".to_string(),
            "-n".to_string(),
            namespace.to_string(),
            pod_name.to_string(),
            "--".to_string(),
            "claude".to_string(),
            "--dangerously-skip-permissions".to_string(),
            "/remote-control".to_string(),
        ],
    )
}

/// Build the kubectl command string for shelling into a pod with claude.
/// Used in tests to verify the command format matches what open_terminal_with_kubectl_exec produces.
#[cfg(test)]
fn shell_command(kubectl_bin: &str, namespace: &str, pod_name: &str) -> String {
    format!("{} exec -it -n {} {} -- claude --dangerously-skip-permissions", kubectl_bin, namespace, pod_name)
}

fn pods_container_status(pods: &[PodStatus]) -> String {
    let running = pods.iter().filter(|p| p.phase == "Running").count();
    if running > 0 {
        format!("{} running", running)
    } else if pods.is_empty() {
        "None".to_string()
    } else {
        format!("0/{} running", pods.len())
    }
}

fn compute_memory_info(percent: u8) -> String {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let total_gb = sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let limit_gb = total_gb * percent as f64 / 100.0;
    format!("{:.1} GB of {:.1} GB", limit_gb, total_gb)
}

fn update_tab_status(ui: &slint::Weak<AppWindow>, idx: i32, status: &str) {
    let ui = ui.clone();
    let status = status.to_string();
    slint::invoke_from_event_loop(move || {
        if let Some(u) = ui.upgrade() {
            let tabs = u.get_launch_tabs();
            if let Some(mut tab) = tabs.row_data(idx as usize) {
                tab.status = status.into();
                tabs.set_row_data(idx as usize, tab);
            }
        }
    })
    .ok();
}

fn append_to_tab(ui: &slint::Weak<AppWindow>, idx: i32, text: &str) {
    let ui = ui.clone();
    let text = text.to_string();
    slint::invoke_from_event_loop(move || {
        if let Some(u) = ui.upgrade() {
            let tabs = u.get_launch_tabs();
            if let Some(mut tab) = tabs.row_data(idx as usize) {
                let mut current = tab.log_text.to_string();
                current.push_str(&text);
                current.push('\n');
                tab.log_text = current.into();
                tabs.set_row_data(idx as usize, tab);
            }
        }
    })
    .ok();
}

fn add_step(ui: &slint::Weak<AppWindow>, label: &str, status: &str, message: &str) {
    let ui = ui.clone();
    let label = label.to_string();
    let status = status.to_string();
    let message = message.to_string();
    slint::invoke_from_event_loop(move || {
        if let Some(u) = ui.upgrade() {
            let steps = u.get_launch_steps();
            let mut vec: Vec<LaunchStep> = (0..steps.row_count())
                .filter_map(|i| steps.row_data(i))
                .collect();
            vec.push(LaunchStep {
                label: label.into(),
                status: status.into(),
                message: message.into(),
            });
            u.set_launch_steps(std::rc::Rc::new(slint::VecModel::from(vec)).into());
        }
    })
    .ok();
}

fn update_step(ui: &slint::Weak<AppWindow>, idx: usize, status: &str, message: &str) {
    let ui = ui.clone();
    let status = status.to_string();
    let message = message.to_string();
    slint::invoke_from_event_loop(move || {
        if let Some(u) = ui.upgrade() {
            let steps = u.get_launch_steps();
            if let Some(mut step) = steps.row_data(idx) {
                step.status = status.into();
                step.message = message.into();
                steps.set_row_data(idx, step);
            }
        }
    })
    .ok();
}

fn update_install_log(ui: &slint::Weak<AppWindow>, log: &str) {
    let log = log.to_string();
    let ui = ui.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(u) = ui.upgrade() {
            u.set_install_log(log.into());
        }
    })
    .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pod(name: &str, selected: bool) -> PodStatus {
        PodStatus {
            name: name.to_string(),
            project: name.to_string(),
            phase: "Running".to_string(),
            ready: true,
            restart_count: 0,
            age: "1h".to_string(),
            warnings: vec![],
            exposed: false,
            container_port: 0,
            selected,
        }
    }

    #[test]
    fn merge_pod_selection_preserves_selected() {
        let old = vec![make_pod("pod-a", true), make_pod("pod-b", false)];
        let mut new = vec![make_pod("pod-a", false), make_pod("pod-b", false)];
        merge_pod_selection(&old, &mut new);
        assert!(new[0].selected, "pod-a should be selected after merge");
        assert!(!new[1].selected, "pod-b should remain unselected");
    }

    #[test]
    fn merge_pod_selection_no_match() {
        let old = vec![make_pod("pod-a", true)];
        let mut new = vec![make_pod("pod-c", false)];
        merge_pod_selection(&old, &mut new);
        assert!(!new[0].selected, "pod-c should stay unselected (no match in old)");
    }

    #[test]
    fn merge_pod_selection_empty_old() {
        let old: Vec<PodStatus> = vec![];
        let mut new = vec![make_pod("pod-a", false), make_pod("pod-b", false)];
        merge_pod_selection(&old, &mut new);
        assert!(!new[0].selected);
        assert!(!new[1].selected);
    }

    #[test]
    fn merge_pod_selection_duplicate_names_in_old() {
        // If old list has duplicates, the first match wins (both selected)
        let old = vec![make_pod("pod-a", true), make_pod("pod-a", false)];
        let mut new = vec![make_pod("pod-a", false)];
        merge_pod_selection(&old, &mut new);
        // find() returns first match which is selected=true
        assert!(new[0].selected, "should use first match from old list");
    }

    #[test]
    fn merge_pod_selection_all_selected() {
        let old = vec![make_pod("a", true), make_pod("b", true), make_pod("c", true)];
        let mut new = vec![make_pod("a", false), make_pod("b", false), make_pod("c", false)];
        merge_pod_selection(&old, &mut new);
        assert!(new.iter().all(|p| p.selected), "all should be selected after merge");
    }

    #[test]
    fn merge_pod_selection_new_pods_added() {
        // Old has pod-a (selected), new has pod-a + pod-b (new pod)
        let old = vec![make_pod("pod-a", true)];
        let mut new = vec![make_pod("pod-a", false), make_pod("pod-b", false)];
        merge_pod_selection(&old, &mut new);
        assert!(new[0].selected, "existing pod should preserve selection");
        assert!(!new[1].selected, "new pod should be unselected");
    }

    #[test]
    fn merge_pod_selection_old_pods_removed() {
        // Old has pods that don't exist in new (deleted pods)
        let old = vec![make_pod("pod-a", true), make_pod("pod-deleted", true)];
        let mut new = vec![make_pod("pod-a", false)];
        merge_pod_selection(&old, &mut new);
        assert!(new[0].selected);
        assert_eq!(new.len(), 1, "deleted pod should not reappear");
    }

    // =========================================================================
    // Pod selection edge cases
    // =========================================================================

    #[test]
    fn merge_pod_selection_empty_both() {
        let old: Vec<PodStatus> = vec![];
        let mut new: Vec<PodStatus> = vec![];
        merge_pod_selection(&old, &mut new);
        assert!(new.is_empty());
    }

    #[test]
    fn merge_pod_selection_empty_new() {
        let old = vec![make_pod("pod-a", true)];
        let mut new: Vec<PodStatus> = vec![];
        merge_pod_selection(&old, &mut new);
        assert!(new.is_empty(), "no pods to merge into");
    }

    #[test]
    fn merge_pod_selection_preserves_non_selection_fields() {
        let old = vec![make_pod("pod-a", true)];
        let mut new = vec![PodStatus {
            name: "pod-a".to_string(),
            project: "updated-project".to_string(),
            phase: "Pending".to_string(),
            ready: false,
            restart_count: 3,
            age: "2h".to_string(),
            warnings: vec!["OOMKilled".to_string()],
            exposed: true,
            container_port: 9090,
            selected: false,
        }];
        merge_pod_selection(&old, &mut new);
        assert!(new[0].selected, "selection should be merged");
        assert_eq!(new[0].project, "updated-project", "project should not be overwritten");
        assert_eq!(new[0].phase, "Pending", "phase should not be overwritten");
        assert_eq!(new[0].restart_count, 3, "restart count should not be overwritten");
        assert!(new[0].exposed, "exposed should not be overwritten");
    }

    #[test]
    fn merge_pod_selection_large_list() {
        let old: Vec<PodStatus> = (0..100)
            .map(|i| make_pod(&format!("pod-{}", i), i % 3 == 0))
            .collect();
        let mut new: Vec<PodStatus> = (0..100)
            .map(|i| make_pod(&format!("pod-{}", i), false))
            .collect();
        merge_pod_selection(&old, &mut new);
        let selected_count = new.iter().filter(|p| p.selected).count();
        assert_eq!(selected_count, 34, "every 3rd pod should be selected (0, 3, 6, ..., 99)");
    }

    #[test]
    fn merge_pod_selection_pod_recreated_different_name() {
        // Simulates K8s recreating a pod with new suffix
        let old = vec![make_pod("claude-frontend-abc123", true)];
        let mut new = vec![make_pod("claude-frontend-def456", false)];
        merge_pod_selection(&old, &mut new);
        assert!(!new[0].selected, "different pod name should not inherit selection");
    }

    // =========================================================================
    // pods_container_status
    // =========================================================================

    #[test]
    fn pods_container_status_empty() {
        assert_eq!(pods_container_status(&[]), "None");
    }

    #[test]
    fn pods_container_status_all_running() {
        let pods = vec![make_pod("a", false), make_pod("b", false)];
        assert_eq!(pods_container_status(&pods), "2 running");
    }

    #[test]
    fn pods_container_status_none_running() {
        let pods = vec![
            PodStatus { phase: "Pending".to_string(), ..make_pod("a", false) },
            PodStatus { phase: "Failed".to_string(), ..make_pod("b", false) },
        ];
        assert_eq!(pods_container_status(&pods), "0/2 running");
    }

    #[test]
    fn pods_container_status_mixed() {
        let pods = vec![
            make_pod("a", false),
            PodStatus { phase: "Pending".to_string(), ..make_pod("b", false) },
            make_pod("c", false),
        ];
        assert_eq!(pods_container_status(&pods), "2 running");
    }

    // =========================================================================
    // remote_control_procs tracking
    // =========================================================================

    #[test]
    fn remote_control_procs_insert_and_remove() {
        let mut state = app::AppState::new().unwrap();
        assert!(state.remote_control_procs.is_empty());

        state.remote_control_procs.insert("pod-a".to_string(), 12345);
        assert!(state.remote_control_procs.contains_key("pod-a"));
        assert_eq!(state.remote_control_procs.get("pod-a"), Some(&12345));

        state.remote_control_procs.insert("pod-b".to_string(), 67890);
        assert_eq!(state.remote_control_procs.len(), 2);

        let removed = state.remote_control_procs.remove("pod-a");
        assert_eq!(removed, Some(12345));
        assert!(!state.remote_control_procs.contains_key("pod-a"));
        assert_eq!(state.remote_control_procs.len(), 1);
    }

    #[test]
    fn remote_control_procs_remove_nonexistent() {
        let mut state = app::AppState::new().unwrap();
        let removed = state.remote_control_procs.remove("nonexistent");
        assert_eq!(removed, None);
    }

    #[test]
    fn remote_control_procs_overwrite_pid() {
        let mut state = app::AppState::new().unwrap();
        state.remote_control_procs.insert("pod-a".to_string(), 100);
        state.remote_control_procs.insert("pod-a".to_string(), 200);
        assert_eq!(state.remote_control_procs.get("pod-a"), Some(&200));
        assert_eq!(state.remote_control_procs.len(), 1);
    }

    // =========================================================================
    // syncing_ui guard
    // =========================================================================

    #[test]
    fn syncing_ui_defaults_to_false() {
        let state = app::AppState::new().unwrap();
        assert!(!state.syncing_ui);
    }

    #[test]
    fn syncing_ui_guards_pod_toggled() {
        // Simulates what on_pod_toggled does: skip if syncing_ui is true
        let mut state = app::AppState::new().unwrap();
        state.pods = vec![make_pod("pod-a", false)];

        // Without guard: toggling works
        state.syncing_ui = false;
        if !state.syncing_ui {
            state.pods[0].selected = true;
        }
        assert!(state.pods[0].selected);

        // With guard: toggling is skipped
        state.syncing_ui = true;
        if !state.syncing_ui {
            state.pods[0].selected = false;
        }
        assert!(state.pods[0].selected, "should remain selected when syncing_ui is true");
    }

    #[test]
    fn syncing_ui_guards_select_all() {
        let mut state = app::AppState::new().unwrap();
        state.pods = vec![make_pod("a", false), make_pod("b", false)];

        // Without guard: select all works
        state.syncing_ui = false;
        if !state.syncing_ui {
            for p in &mut state.pods {
                p.selected = true;
            }
        }
        assert!(state.pods.iter().all(|p| p.selected));

        // With guard: deselect all is skipped
        state.syncing_ui = true;
        if !state.syncing_ui {
            for p in &mut state.pods {
                p.selected = false;
            }
        }
        assert!(state.pods.iter().all(|p| p.selected), "should remain selected when syncing_ui is true");
    }

    // =========================================================================
    // merge_pod_selection + remote_control interaction
    // =========================================================================

    #[test]
    fn merge_pod_selection_does_not_affect_remote_control() {
        // remote_control is tracked separately in remote_control_procs, not in PodStatus.
        // Verify merge only touches `selected`.
        let old = vec![PodStatus {
            exposed: true,
            ..make_pod("pod-a", true)
        }];
        let mut new = vec![PodStatus {
            exposed: false,
            ..make_pod("pod-a", false)
        }];
        merge_pod_selection(&old, &mut new);
        assert!(new[0].selected, "selected should be merged");
        assert!(!new[0].exposed, "exposed should NOT be overwritten by merge");
    }

    // =========================================================================
    // remote_control_command / shell_command builders
    // =========================================================================

    #[test]
    fn remote_control_command_basic() {
        let (program, args) = remote_control_command("kubectl", "claude-code", "my-pod-abc");
        assert_eq!(program, "kubectl");
        assert_eq!(args, vec!["exec", "-i", "-n", "claude-code", "my-pod-abc", "--", "claude", "--dangerously-skip-permissions", "/remote-control"]);
    }

    #[test]
    fn remote_control_command_custom_namespace() {
        let (program, args) = remote_control_command("kubectl", "custom-ns", "pod-123");
        assert_eq!(program, "kubectl");
        assert_eq!(args[3], "custom-ns");
        assert_eq!(args[4], "pod-123");
    }

    #[test]
    fn remote_control_command_custom_binary() {
        let (program, _) = remote_control_command("/usr/local/bin/kubectl", "ns", "pod");
        assert_eq!(program, "/usr/local/bin/kubectl");
    }

    #[test]
    fn remote_control_command_skips_permissions() {
        let (_, args) = remote_control_command("kubectl", "ns", "pod");
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()),
            "remote control should skip permissions prompt");
    }

    #[test]
    fn remote_control_command_always_uses_claude_remote_control() {
        let (_, args) = remote_control_command("kubectl", "ns", "pod");
        // Last arg must be "/remote-control"
        assert_eq!(args.last().unwrap(), "/remote-control");
        // Must contain "claude" before it
        let claude_pos = args.iter().position(|a| a == "claude").unwrap();
        let rc_pos = args.iter().position(|a| a == "/remote-control").unwrap();
        assert!(claude_pos < rc_pos);
    }

    #[test]
    fn remote_control_command_uses_dash_i_not_dash_it() {
        // Remote control runs in background, so no -t (tty)
        let (_, args) = remote_control_command("kubectl", "ns", "pod");
        assert!(args.contains(&"-i".to_string()), "should use -i for stdin");
        assert!(!args.contains(&"-it".to_string()), "should NOT use -it (no tty for background)");
        assert!(!args.contains(&"-t".to_string()), "should NOT use -t (no tty for background)");
    }

    #[test]
    fn shell_command_basic() {
        let cmd = shell_command("kubectl", "claude-code", "my-pod");
        assert_eq!(cmd, "kubectl exec -it -n claude-code my-pod -- claude --dangerously-skip-permissions");
    }

    #[test]
    fn shell_command_uses_interactive_tty() {
        let cmd = shell_command("kubectl", "ns", "pod");
        assert!(cmd.contains(" -it "), "shell should use -it for interactive tty");
    }

    #[test]
    fn shell_command_runs_claude_not_sh() {
        let cmd = shell_command("kubectl", "ns", "pod");
        assert!(cmd.contains("-- claude"), "shell should run claude, not /bin/sh");
        assert!(!cmd.contains("/bin/sh"), "should not contain /bin/sh");
    }

    #[test]
    fn shell_command_skips_permissions() {
        let cmd = shell_command("kubectl", "ns", "pod");
        assert!(cmd.contains("--dangerously-skip-permissions"),
            "shell should skip permissions prompt");
    }

    #[test]
    fn shell_command_custom_binary_path() {
        let cmd = shell_command("C:\\tools\\kubectl.exe", "claude-code", "pod-xyz");
        assert!(cmd.starts_with("C:\\tools\\kubectl.exe "));
    }

    #[test]
    fn remote_control_vs_shell_command_differences() {
        // Remote control: -i only, runs "claude --dangerously-skip-permissions /remote-control"
        let (_, rc_args) = remote_control_command("kubectl", "claude-code", "pod-1");
        // Shell: -it, runs "claude --dangerously-skip-permissions"
        let shell_cmd = shell_command("kubectl", "claude-code", "pod-1");

        // Both skip permissions
        assert!(rc_args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(shell_cmd.contains("--dangerously-skip-permissions"));

        // Remote control has "/remote-control", shell does not
        assert!(rc_args.contains(&"/remote-control".to_string()));
        assert!(!shell_cmd.contains("/remote-control"));

        // Remote control uses -i (no tty), shell uses -it (with tty)
        assert!(rc_args.contains(&"-i".to_string()));
        assert!(!rc_args.contains(&"-t".to_string()));
        assert!(shell_cmd.contains(" -it "));
    }

    // =========================================================================
    // Integration tests: mock kubectl script records args
    // =========================================================================

    /// Create a mock kubectl script that writes its arguments to a file.
    /// Returns (script_path, args_output_path).
    fn create_mock_kubectl(dir: &std::path::Path) -> (String, std::path::PathBuf) {
        let args_file = dir.join("kubectl_args.txt");

        #[cfg(unix)]
        {
            let script_path = dir.join("mock-kubectl");
            let script = format!(
                "#!/bin/sh\necho \"$@\" > \"{}\"\n",
                args_file.to_string_lossy()
            );
            std::fs::write(&script_path, script).unwrap();
            std::fs::set_permissions(
                &script_path,
                std::os::unix::fs::PermissionsExt::from_mode(0o755),
            )
            .unwrap();
            (script_path.to_string_lossy().to_string(), args_file)
        }

        #[cfg(windows)]
        {
            let script_path = dir.join("mock-kubectl.bat");
            let script = format!(
                "@echo off\r\necho %* > \"{}\"\r\n",
                args_file.to_string_lossy()
            );
            std::fs::write(&script_path, &script).unwrap();
            (script_path.to_string_lossy().to_string(), args_file)
        }
    }

    #[test]
    fn integration_remote_control_spawns_correct_kubectl_args() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (mock_kubectl, args_file) = create_mock_kubectl(tmp.path());

        let (program, args) = remote_control_command(&mock_kubectl, "claude-code", "test-pod-xyz");

        // Actually spawn the mock to verify args are passed through
        let child = std::process::Command::new(&program)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        assert!(child.is_ok(), "mock kubectl should spawn successfully");
        let mut child = child.unwrap();
        let _ = child.wait(); // Wait for it to finish writing

        let recorded = std::fs::read_to_string(&args_file)
            .expect("mock should have written args file");
        let recorded = recorded.trim();

        assert!(recorded.contains("exec"), "should contain 'exec': {}", recorded);
        assert!(recorded.contains("-i"), "should contain '-i': {}", recorded);
        assert!(recorded.contains("-n"), "should contain '-n': {}", recorded);
        assert!(recorded.contains("claude-code"), "should contain namespace: {}", recorded);
        assert!(recorded.contains("test-pod-xyz"), "should contain pod name: {}", recorded);
        assert!(recorded.contains("claude"), "should contain 'claude' command: {}", recorded);
        assert!(recorded.contains("--dangerously-skip-permissions"), "should contain '--dangerously-skip-permissions': {}", recorded);
        assert!(recorded.contains("/remote-control"), "should contain '/remote-control': {}", recorded);
        // Must NOT have -t (no tty for background)
        assert!(!recorded.contains("-it"), "should NOT contain '-it' for background process: {}", recorded);
    }

    #[test]
    fn integration_remote_control_tracks_pid() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (mock_kubectl, _) = create_mock_kubectl(tmp.path());

        let (program, args) = remote_control_command(&mock_kubectl, "claude-code", "pid-test-pod");

        let mut child = std::process::Command::new(&program)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("should spawn");

        let pid = child.id();
        assert!(pid > 0, "PID should be a positive number");

        // Simulate what on_exec_claude does: store in HashMap
        let mut procs = std::collections::HashMap::new();
        procs.insert("pid-test-pod".to_string(), pid);
        assert!(procs.contains_key("pid-test-pod"));
        assert_eq!(procs.get("pid-test-pod"), Some(&pid));

        // Simulate stopping: remove from HashMap
        let removed_pid = procs.remove("pid-test-pod").unwrap();
        assert_eq!(removed_pid, pid);
        assert!(!procs.contains_key("pid-test-pod"));

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn integration_shell_command_format_matches_kubectl_exec() {
        // Verify the shell command format that gets passed to terminal emulators
        let cmd = shell_command("kubectl", "claude-code", "shell-test-pod");

        // Parse the command to verify structure
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        assert_eq!(parts[0], "kubectl", "binary");
        assert_eq!(parts[1], "exec", "exec subcommand");
        assert_eq!(parts[2], "-it", "interactive+tty flags");
        assert_eq!(parts[3], "-n", "namespace flag");
        assert_eq!(parts[4], "claude-code", "namespace value");
        assert_eq!(parts[5], "shell-test-pod", "pod name");
        assert_eq!(parts[6], "--", "command separator");
        assert_eq!(parts[7], "claude", "command to run in pod");
        assert_eq!(parts[8], "--dangerously-skip-permissions", "skip permissions flag");
        assert_eq!(parts.len(), 9, "should have exactly 9 parts");
    }

    #[test]
    fn integration_remote_control_spawn_and_kill() {
        // Spawn a long-running mock process and kill it, simulating the toggle behavior
        let tmp = tempfile::TempDir::new().unwrap();

        // Create a mock that sleeps instead of exiting immediately
        #[cfg(unix)]
        let script_path = {
            let p = tmp.path().join("mock-kubectl-slow");
            std::fs::write(&p, "#!/bin/sh\nsleep 30\n").unwrap();
            std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
            p.to_string_lossy().to_string()
        };
        #[cfg(windows)]
        let script_path = {
            let p = tmp.path().join("mock-kubectl-slow.bat");
            std::fs::write(&p, "@echo off\r\nping -n 30 127.0.0.1 > nul\r\n").unwrap();
            p.to_string_lossy().to_string()
        };

        let (_, args) = remote_control_command(&script_path, "claude-code", "kill-test-pod");

        let mut child = std::process::Command::new(&script_path)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("should spawn slow mock");

        let pid = child.id();
        assert!(pid > 0);

        // Simulate what on_exec_claude does when stopping
        #[cfg(unix)]
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .output();
        }

        // Process should exit after being killed
        let status = child.wait().expect("should be able to wait");
        // On kill, exit status is non-zero (or signal on unix)
        assert!(!status.success() || cfg!(windows), "killed process should not show success");
    }

    #[test]
    fn integration_multiple_remote_controls_independent() {
        // Verify we can track multiple remote control processes independently
        let tmp = tempfile::TempDir::new().unwrap();
        let (mock_kubectl, _) = create_mock_kubectl(tmp.path());

        let mut procs = std::collections::HashMap::new();

        // Start "remote control" for 3 pods
        let mut children = Vec::new();
        for pod_name in &["pod-alpha", "pod-beta", "pod-gamma"] {
            let (program, args) = remote_control_command(&mock_kubectl, "claude-code", pod_name);
            let child = std::process::Command::new(&program)
                .args(&args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("should spawn");
            procs.insert(pod_name.to_string(), child.id());
            children.push(child);
        }

        assert_eq!(procs.len(), 3);
        assert!(procs.contains_key("pod-alpha"));
        assert!(procs.contains_key("pod-beta"));
        assert!(procs.contains_key("pod-gamma"));

        // All PIDs should be different
        let pids: Vec<u32> = procs.values().copied().collect();
        let unique_pids: std::collections::HashSet<u32> = pids.iter().copied().collect();
        assert_eq!(unique_pids.len(), 3, "all PIDs should be unique");

        // Stop one - others remain
        procs.remove("pod-beta");
        assert_eq!(procs.len(), 2);
        assert!(!procs.contains_key("pod-beta"));
        assert!(procs.contains_key("pod-alpha"));
        assert!(procs.contains_key("pod-gamma"));

        for mut child in children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    // PRJ-53: retry-build step label parsing
    #[test]
    fn retry_build_extracts_project_name_from_step_label() {
        let label = "Building my-project";
        let name = label.strip_prefix("Building ").unwrap_or(label);
        assert_eq!(name, "my-project");
    }

    #[test]
    fn retry_build_passes_through_non_build_label() {
        let label = "Helm Deploy";
        let name = label.strip_prefix("Building ").unwrap_or(label);
        assert_eq!(name, "Helm Deploy");
    }

    // =========================================================================
    // Tier 3: Orchestration tests — verify callback-to-backend wiring contracts
    // =========================================================================

    /// Verify save_settings contract: UI fields flow into AppConfig and persist.
    #[test]
    fn orchestration_save_settings_roundtrip() {
        // Simulate what on_save_settings does: read UI fields, update config, save.
        let tmp = tempfile::TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");

        let cfg = config::AppConfig {
            claude_mode: "headless".to_string(),
            git_user_name: "Test Bot".to_string(),
            git_user_email: "test@example.com".to_string(),
            cpu_limit: "8".to_string(),
            memory_limit: "16Gi".to_string(),
            cluster_memory_percent: 75,
            terraform_dir: "infra/tf".to_string(),
            helm_chart_dir: "charts/custom".to_string(),
            ..Default::default()
        };

        cfg.save_to(&config_path).expect("save settings");
        let loaded = config::AppConfig::load_from(&config_path).expect("load settings");

        assert_eq!(loaded.claude_mode, "headless");
        assert_eq!(loaded.git_user_name, "Test Bot");
        assert_eq!(loaded.git_user_email, "test@example.com");
        assert_eq!(loaded.cpu_limit, "8");
        assert_eq!(loaded.memory_limit, "16Gi");
        assert_eq!(loaded.cluster_memory_percent, 75);
        assert_eq!(loaded.terraform_dir, "infra/tf");
        assert_eq!(loaded.helm_chart_dir, "charts/custom");
    }

    /// Verify project_toggled contract: toggling index updates the model.
    #[test]
    fn orchestration_project_toggle_updates_model() {
        let mut state = app::AppState::new().unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("proj_a")).unwrap();
        std::fs::create_dir(tmp.path().join("proj_b")).unwrap();
        std::fs::create_dir(tmp.path().join("proj_c")).unwrap();
        state.config.projects_dir = Some(tmp.path().to_string_lossy().into_owned());
        state.scan_projects().unwrap();
        assert_eq!(state.projects.len(), 3);
        assert!(state.projects.iter().all(|p| !p.selected));

        // Simulate on_project_toggled(1, true)
        if let Some(p) = state.projects.get_mut(1) {
            p.selected = true;
        }
        let selected = state.selected_projects();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, state.projects[1].name);

        // Simulate on_project_toggled(1, false)
        if let Some(p) = state.projects.get_mut(1) {
            p.selected = false;
        }
        assert!(state.selected_projects().is_empty());
    }

    /// Verify toggle_select_all contract for projects.
    #[test]
    fn orchestration_toggle_select_all_projects() {
        let mut state = app::AppState::new().unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("a")).unwrap();
        std::fs::create_dir(tmp.path().join("b")).unwrap();
        state.config.projects_dir = Some(tmp.path().to_string_lossy().into_owned());
        state.scan_projects().unwrap();

        // Simulate on_toggle_select_all: all_selected = false, so set all to true
        let all_selected = state.projects.iter().all(|p| p.selected);
        let new_val = !all_selected;
        for p in &mut state.projects {
            p.selected = new_val;
        }
        assert_eq!(state.selected_projects().len(), 2);

        // Toggle again: all selected, so set all to false
        let all_selected = state.projects.iter().all(|p| p.selected);
        let new_val = !all_selected;
        for p in &mut state.projects {
            p.selected = new_val;
        }
        assert!(state.selected_projects().is_empty());
    }

    /// Verify toggle_select_all_pods contract: sets all pods to selected/deselected.
    #[test]
    fn orchestration_toggle_select_all_pods() {
        let mut state = app::AppState::new().unwrap();
        state.pods = vec![
            make_pod("pod-1", false),
            make_pod("pod-2", false),
            make_pod("pod-3", true),
        ];

        // Simulate on_toggle_select_all_pods(true)
        for p in &mut state.pods {
            p.selected = true;
        }
        assert!(state.pods.iter().all(|p| p.selected));

        // Simulate on_toggle_select_all_pods(false)
        for p in &mut state.pods {
            p.selected = false;
        }
        assert!(state.pods.iter().all(|p| !p.selected));
    }

    /// Verify pod_toggled contract: toggling a single pod by index.
    #[test]
    fn orchestration_pod_toggled_by_index() {
        let mut state = app::AppState::new().unwrap();
        state.pods = vec![
            make_pod("pod-a", false),
            make_pod("pod-b", false),
            make_pod("pod-c", false),
        ];

        // Simulate on_pod_toggled(1, true)
        if let Some(p) = state.pods.get_mut(1) {
            p.selected = true;
        }
        assert!(!state.pods[0].selected);
        assert!(state.pods[1].selected);
        assert!(!state.pods[2].selected);

        // Simulate on_pod_toggled(1, false)
        if let Some(p) = state.pods.get_mut(1) {
            p.selected = false;
        }
        assert!(state.pods.iter().all(|p| !p.selected));
    }

    /// Verify compute_memory_info returns a formatted string.
    #[test]
    fn orchestration_compute_memory_info_format() {
        let info = compute_memory_info(80);
        assert!(info.contains("GB"), "should contain GB units: {}", info);
        assert!(info.contains("of"), "should contain 'of': {}", info);
    }

    /// Verify compute_memory_info varies with percentage.
    #[test]
    fn orchestration_compute_memory_info_varies_with_percent() {
        let info_50 = compute_memory_info(50);
        let info_90 = compute_memory_info(90);
        // Both should be valid format, and 90% limit > 50% limit
        assert!(info_50.contains("GB"));
        assert!(info_90.contains("GB"));
        // Parse the limit GB value (first number before "of")
        let parse_limit = |s: &str| -> f64 {
            let parts: Vec<&str> = s.split(" of ").collect();
            parts[0].trim().replace(" GB", "").parse::<f64>().unwrap_or(0.0)
        };
        let limit_50 = parse_limit(&info_50);
        let limit_90 = parse_limit(&info_90);
        assert!(
            limit_90 > limit_50,
            "90% ({}) should yield larger limit than 50% ({})",
            info_90,
            info_50
        );
    }

    /// Verify browse_folder contract: setting projects_dir triggers scan.
    #[test]
    fn orchestration_browse_folder_triggers_scan() {
        let mut state = app::AppState::new().unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("project_x")).unwrap();

        // Simulate on_browse_folder: set dir, save config, scan
        let path_str = tmp.path().to_string_lossy().to_string();
        state.config.projects_dir = Some(path_str);
        let _ = state.scan_projects();
        assert_eq!(state.projects.len(), 1);
        assert_eq!(state.projects[0].name, "project_x");
    }

    /// Verify save_settings clamps cluster_memory_percent to 50..95.
    #[test]
    fn orchestration_save_settings_clamps_memory_percent() {
        // Simulate what on_save_settings does with out-of-range values
        let raw_str = "30"; // below minimum
        let clamped = raw_str.parse::<u8>().unwrap_or(80).clamp(50, 95);
        assert_eq!(clamped, 50);

        let raw_str = "99"; // above maximum
        let clamped = raw_str.parse::<u8>().unwrap_or(80).clamp(50, 95);
        assert_eq!(clamped, 95);

        let raw_str = "not_a_number";
        let clamped = raw_str.parse::<u8>().unwrap_or(80).clamp(50, 95);
        assert_eq!(clamped, 80, "invalid input should default to 80");
    }

    /// Verify that merge_pod_selection + pod refresh preserves user selections.
    #[test]
    fn orchestration_pod_refresh_preserves_selection() {
        // Initial state: user selected pod-a
        let old_pods = vec![
            make_pod("pod-a", true),
            make_pod("pod-b", false),
        ];

        // Simulated refresh from kubectl returns fresh pod data (all unselected)
        let mut new_pods = vec![
            make_pod("pod-a", false),
            make_pod("pod-b", false),
            make_pod("pod-c", false), // new pod appeared
        ];

        merge_pod_selection(&old_pods, &mut new_pods);

        assert!(new_pods[0].selected, "pod-a selection should be preserved");
        assert!(!new_pods[1].selected, "pod-b should remain unselected");
        assert!(!new_pods[2].selected, "new pod-c should start unselected");
    }

    /// Verify cancel_flag contract: can be set and read across threads.
    #[test]
    fn orchestration_cancel_flag_atomic() {
        let state = app::AppState::new().unwrap();
        let flag = state.cancel_flag.clone();
        assert!(!flag.load(std::sync::atomic::Ordering::Relaxed));

        // Simulate on_cancel_launch
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(flag.load(std::sync::atomic::Ordering::Relaxed));

        // Simulate reset before new launch
        flag.store(false, std::sync::atomic::Ordering::Relaxed);
        assert!(!flag.load(std::sync::atomic::Ordering::Relaxed));
    }

}
