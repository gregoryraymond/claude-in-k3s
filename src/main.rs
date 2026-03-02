mod app;
mod config;
mod docker;
mod error;
mod helm;
mod kubectl;
mod platform;
mod projects;
mod terraform;

use app::AppState;
use projects::BaseImage;
use std::sync::{Arc, Mutex};

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("claude_in_k3s=info".parse().unwrap()),
        )
        .init();

    let rt = tokio::runtime::Runtime::new()?;

    let state = Arc::new(Mutex::new(AppState::new()?));

    let ui = AppWindow::new()?;

    // Set initial UI state
    {
        let s = state.lock().unwrap();
        ui.set_platform_name(platform::platform_display_name(&s.platform).into());
        if let Some(ref dir) = s.config.projects_dir {
            ui.set_projects_dir(dir.into());
        }
        if let Some(ref key) = s.config.api_key {
            ui.set_api_key(key.into());
        }
        ui.set_tf_initialized(s.terraform_runner().is_initialized());
    }

    // --- Terraform callbacks ---

    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_terraform_init(move || {
            let ui = ui_handle.clone();
            let state = state.clone();
            set_busy(&ui, true);
            append_log(&state, "Running terraform init...");
            sync_log(&ui, &state);

            rt_handle.spawn(async move {
                let runner = {
                    let s = state.lock().unwrap();
                    s.terraform_runner()
                };
                let result = runner.init().await;

                slint::invoke_from_event_loop(move || {
                    match result {
                        Ok(r) => {
                            append_log(&state, &format_cmd_result("terraform init", &r));
                            if r.success {
                                state.lock().unwrap().tf_initialized = true;
                                if let Some(ui) = ui.upgrade() {
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

        ui.on_terraform_apply(move || {
            let ui = ui_handle.clone();
            let state = state.clone();
            set_busy(&ui, true);
            append_log(&state, "Running terraform apply...");
            sync_log(&ui, &state);

            rt_handle.spawn(async move {
                let runner = {
                    let s = state.lock().unwrap();
                    s.terraform_runner()
                };
                let result = runner.apply().await;

                slint::invoke_from_event_loop(move || {
                    match result {
                        Ok(r) => {
                            append_log(&state, &format_cmd_result("terraform apply", &r));
                            if r.success {
                                state.lock().unwrap().cluster_healthy = true;
                                if let Some(ui) = ui.upgrade() {
                                    ui.set_cluster_status("Healthy".into());
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
                            append_log(&state, &format_cmd_result("terraform destroy", &r));
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
                            append_log(&state, &format_cmd_result("terraform plan", &r));
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
                    let _ = s.scan_projects();
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
                let _ = s.scan_projects();
            }
            sync_projects(&ui_handle, &state);
        });
    }

    // --- Project toggled ---
    {
        let state = state.clone();

        ui.on_project_toggled(move |idx, checked| {
            let mut s = state.lock().unwrap();
            if let Some(p) = s.projects.get_mut(idx as usize) {
                p.selected = checked;
            }
        });
    }

    // --- Project image changed ---
    {
        let state = state.clone();

        ui.on_project_image_changed(move |idx, img_idx| {
            let mut s = state.lock().unwrap();
            if let Some(p) = s.projects.get_mut(idx as usize) {
                p.base_image = BaseImage::from_index(img_idx);
            }
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
            set_busy(&ui, true);
            append_log(&state, "Building images and deploying selected projects...");
            sync_log(&ui, &state);

            let (api_key, selected_projects, docker_builder, helm_runner) = {
                let s = state.lock().unwrap();
                let api_key = s.config.api_key.clone().unwrap_or_default();
                let projects = s.selected_projects().into_iter().cloned().collect::<Vec<_>>();
                (api_key, projects, s.docker_builder(), s.helm_runner())
            };

            rt_handle.spawn(async move {
                // Build and import Docker images for each selected project
                let mut project_tuples = Vec::new();
                let mut build_failed = false;

                for project in &selected_projects {
                    let tag = docker::image_tag_for_project(project);

                    slint::invoke_from_event_loop({
                        let state = state.clone();
                        let ui = ui.clone();
                        let name = project.name.clone();
                        move || {
                            append_log(&state, &format!("Building image for '{}'...", name));
                            sync_log(&ui, &state);
                        }
                    }).ok();

                    match docker_builder.build_and_import(project).await {
                        Ok(r) if r.success => {
                            project_tuples.push((
                                project.name.clone(),
                                project.path.to_string_lossy().to_string(),
                                tag,
                            ));
                        }
                        Ok(r) => {
                            let msg = format!("Image build failed for '{}': {}", project.name, r.stderr);
                            slint::invoke_from_event_loop({
                                let state = state.clone();
                                let ui = ui.clone();
                                move || {
                                    append_log(&state, &msg);
                                    sync_log(&ui, &state);
                                }
                            }).ok();
                            build_failed = true;
                            break;
                        }
                        Err(e) => {
                            let msg = format!("Image build error for '{}': {}", project.name, e);
                            slint::invoke_from_event_loop({
                                let state = state.clone();
                                let ui = ui.clone();
                                move || {
                                    append_log(&state, &msg);
                                    sync_log(&ui, &state);
                                }
                            }).ok();
                            build_failed = true;
                            break;
                        }
                    }
                }

                if build_failed || project_tuples.is_empty() {
                    slint::invoke_from_event_loop(move || {
                        if !build_failed {
                            append_log(&state, "No projects to deploy.");
                        }
                        set_busy(&ui, false);
                        sync_log(&ui, &state);
                    }).ok();
                    return;
                }

                // Deploy via Helm
                let result = helm_runner
                    .install_or_upgrade(&api_key, &project_tuples)
                    .await;

                slint::invoke_from_event_loop(move || {
                    match result {
                        Ok(r) => {
                            append_log(&state, &format_cmd_result("helm upgrade --install", &r));
                        }
                        Err(e) => append_log(&state, &format!("Deploy error: {}", e)),
                    }
                    set_busy(&ui, false);
                    sync_log(&ui, &state);
                })
                .ok();
            });
        });
    }

    // --- Stop selected ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_stop_selected(move || {
            let ui = ui_handle.clone();
            let state = state.clone();
            set_busy(&ui, true);
            append_log(&state, "Uninstalling Helm release...");
            sync_log(&ui, &state);

            let helm_runner = {
                let s = state.lock().unwrap();
                s.helm_runner()
            };

            rt_handle.spawn(async move {
                let result = helm_runner.uninstall().await;

                slint::invoke_from_event_loop(move || {
                    match result {
                        Ok(r) => {
                            append_log(&state, &format_cmd_result("helm uninstall", &r));
                        }
                        Err(e) => append_log(&state, &format!("Uninstall error: {}", e)),
                    }
                    set_busy(&ui, false);
                    sync_log(&ui, &state);
                })
                .ok();
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
                            append_log(&state, &format_cmd_result("helm status", &r));
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

                slint::invoke_from_event_loop(move || {
                    match result {
                        Ok(pods) => {
                            state.lock().unwrap().pods = pods;
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

    // --- Delete pod ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_delete_pod(move |idx| {
            let ui = ui_handle.clone();
            let state = state.clone();

            let (kubectl, pod_name) = {
                let s = state.lock().unwrap();
                let name = s
                    .pods
                    .get(idx as usize)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                (s.kubectl_runner(), name)
            };

            if pod_name.is_empty() {
                return;
            }

            set_busy(&ui, true);
            append_log(&state, &format!("Deleting pod {}...", pod_name));
            sync_log(&ui, &state);

            rt_handle.spawn(async move {
                let result = kubectl.delete_pod(&pod_name).await;

                slint::invoke_from_event_loop(move || {
                    match result {
                        Ok(r) => {
                            append_log(&state, &format_cmd_result("kubectl delete pod", &r));
                        }
                        Err(e) => append_log(&state, &format!("Delete error: {}", e)),
                    }
                    set_busy(&ui, false);
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

            let (kubectl, pod_name) = {
                let s = state.lock().unwrap();
                let name = s
                    .pods
                    .get(idx as usize)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                (s.kubectl_runner(), name)
            };

            if pod_name.is_empty() {
                return;
            }

            rt_handle.spawn(async move {
                let result = kubectl.get_logs(&pod_name, 100).await;

                slint::invoke_from_event_loop(move || {
                    match result {
                        Ok(r) => {
                            append_log(
                                &state,
                                &format!("--- Logs for {} ---\n{}", pod_name, r.stdout),
                            );
                        }
                        Err(e) => append_log(&state, &format!("Logs error: {}", e)),
                    }
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
                let key = ui.get_api_key().to_string();
                s.config.api_key = if key.is_empty() { None } else { Some(key) };
                match s.config.save() {
                    Ok(_) => s.append_log("Settings saved."),
                    Err(e) => s.append_log(&format!("Failed to save settings: {}", e)),
                }
            }
        });
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

                let kubectl = {
                    let s = state.lock().unwrap();
                    s.kubectl_runner()
                };

                rt_handle.spawn(async move {
                    if let Ok(healthy) = kubectl.cluster_health().await {
                        let pods_result = if healthy {
                            kubectl.get_pods().await.ok()
                        } else {
                            None
                        };

                        slint::invoke_from_event_loop(move || {
                            {
                                let mut s = state.lock().unwrap();
                                s.cluster_healthy = healthy;
                                if let Some(pods) = pods_result {
                                    s.pods = pods;
                                }
                            }

                            if let Some(ui) = ui.upgrade() {
                                let status = if healthy { "Healthy" } else { "Unreachable" };
                                ui.set_cluster_status(status.into());
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

    // Load projects on startup if directory is set
    {
        let mut s = state.lock().unwrap();
        if s.config.projects_dir.is_some() {
            let _ = s.scan_projects();
        }
    }
    sync_projects(&ui.as_weak(), &state);

    ui.run()?;
    Ok(())
}

// --- Helper functions ---

fn set_busy(ui: &slint::Weak<AppWindow>, busy: bool) {
    if let Some(ui) = ui.upgrade() {
        ui.set_is_busy(busy);
    }
}

fn append_log(state: &Arc<Mutex<AppState>>, msg: &str) {
    state.lock().unwrap().append_log(msg);
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
        let entries: Vec<ProjectEntry> = s
            .projects
            .iter()
            .map(|p| ProjectEntry {
                name: p.name.clone().into(),
                path: p.path.to_string_lossy().to_string().into(),
                selected: p.selected,
                base_image_index: p.base_image.to_index(),
                has_custom_dockerfile: p.has_custom_dockerfile,
            })
            .collect();
        let model = std::rc::Rc::new(slint::VecModel::from(entries));
        ui.set_projects(model.into());
    }
}

fn sync_pods(ui: &slint::Weak<AppWindow>, state: &Arc<Mutex<AppState>>) {
    if let Some(ui) = ui.upgrade() {
        let s = state.lock().unwrap();
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
            })
            .collect();
        let model = std::rc::Rc::new(slint::VecModel::from(entries));
        ui.set_pods(model.into());
    }
}

fn format_cmd_result(cmd: &str, r: &error::CmdResult) -> String {
    let status = if r.success { "SUCCESS" } else { "FAILED" };
    let mut out = format!("[{}] {}", status, cmd);
    if !r.stdout.is_empty() {
        out.push_str(&format!("\n{}", r.stdout.trim()));
    }
    if !r.stderr.is_empty() {
        out.push_str(&format!("\nSTDERR: {}", r.stderr.trim()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_cmd_result_success() {
        let r = error::CmdResult {
            success: true,
            stdout: "all good".to_string(),
            stderr: String::new(),
        };
        let output = format_cmd_result("terraform apply", &r);
        assert!(output.contains("[SUCCESS]"), "expected [SUCCESS] in: {}", output);
        assert!(output.contains("terraform apply"), "expected command name in: {}", output);
        assert!(output.contains("all good"), "expected stdout in: {}", output);
    }

    #[test]
    fn format_cmd_result_failure() {
        let r = error::CmdResult {
            success: false,
            stdout: String::new(),
            stderr: "something went wrong".to_string(),
        };
        let output = format_cmd_result("terraform apply", &r);
        assert!(output.contains("[FAILED]"), "expected [FAILED] in: {}", output);
        assert!(
            output.contains("STDERR: something went wrong"),
            "expected stderr in: {}",
            output
        );
    }

    #[test]
    fn format_cmd_result_both_stdout_stderr() {
        let r = error::CmdResult {
            success: true,
            stdout: "some output".to_string(),
            stderr: "some warning".to_string(),
        };
        let output = format_cmd_result("helm install", &r);
        assert!(output.contains("some output"), "expected stdout in: {}", output);
        assert!(
            output.contains("STDERR: some warning"),
            "expected stderr in: {}",
            output
        );
    }

    #[test]
    fn format_cmd_result_empty_output() {
        let r = error::CmdResult {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        };
        let output = format_cmd_result("test cmd", &r);
        assert_eq!(output, "[SUCCESS] test cmd");
    }

    #[test]
    fn format_cmd_result_whitespace_trimmed() {
        let r = error::CmdResult {
            success: false,
            stdout: "  leading and trailing  \n\n".to_string(),
            stderr: "  warn with spaces  \n".to_string(),
        };
        let output = format_cmd_result("cmd", &r);
        assert!(
            output.contains("leading and trailing"),
            "expected trimmed stdout in: {}",
            output
        );
        assert!(
            !output.contains("leading and trailing  \n"),
            "stdout should be trimmed, got: {}",
            output
        );
        assert!(
            output.contains("STDERR: warn with spaces"),
            "expected trimmed stderr in: {}",
            output
        );
        assert!(
            !output.contains("warn with spaces  \n"),
            "stderr should be trimmed, got: {}",
            output
        );
    }
}
