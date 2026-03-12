//! Headless UI tests using Slint testing backend.
//! These tests verify UI property binding, callback wiring, and state management
//! without requiring a display server.
//!
//! All tests run in a single function because the Slint testing backend only
//! supports one initialization per process.

use slint::Model;

slint::include_modules!();

#[test]
fn ui_property_defaults() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    assert_eq!(ui.get_cluster_status(), "Unknown");
    assert_eq!(ui.get_cluster_log(), "");
    assert!(!ui.get_is_busy());
    assert!(!ui.get_tf_initialized());
    assert_eq!(ui.get_projects_dir(), "");
    assert_eq!(ui.get_platform_name(), "Linux");

    // Check defaults for new settings properties
    assert_eq!(ui.get_claude_mode(), "daemon");
    assert_eq!(ui.get_git_user_name(), "Claude Code Bot");
    assert_eq!(ui.get_git_user_email(), "claude-bot@localhost");
    assert_eq!(ui.get_cpu_limit(), "2");
    assert_eq!(ui.get_memory_limit(), "4Gi");
    assert_eq!(ui.get_terraform_dir(), "terraform");
    assert_eq!(ui.get_helm_chart_dir(), "helm/claude-code");

    assert_eq!(ui.get_docker_status(), "Unknown");
    assert_eq!(ui.get_containers_status(), "Unknown");

    // -- new property defaults --
    assert!(!ui.get_all_selected());
    assert!(!ui.get_claude_found());
    assert_eq!(ui.get_claude_version(), "");
    assert_eq!(ui.get_active_launch_tab(), 0);

    // -- set/get cluster status --
    ui.set_cluster_status("Healthy".into());
    assert_eq!(ui.get_cluster_status(), "Healthy");
    ui.set_cluster_status("Unreachable".into());
    assert_eq!(ui.get_cluster_status(), "Unreachable");

    // -- set/get busy state --
    ui.set_is_busy(true);
    assert!(ui.get_is_busy());
    ui.set_is_busy(false);
    assert!(!ui.get_is_busy());

    // -- set/get tf_initialized --
    ui.set_tf_initialized(true);
    assert!(ui.get_tf_initialized());
    ui.set_tf_initialized(false);
    assert!(!ui.get_tf_initialized());

    // -- set/get projects_dir --
    ui.set_projects_dir("/home/user/projects".into());
    assert_eq!(ui.get_projects_dir(), "/home/user/projects");

    // -- set/get platform_name --
    ui.set_platform_name("WSL2".into());
    assert_eq!(ui.get_platform_name(), "WSL2");

    // -- set/get cluster_log --
    ui.set_cluster_log("Running terraform init...\nDone.".into());
    assert_eq!(ui.get_cluster_log(), "Running terraform init...\nDone.");

    // Set/get new settings properties
    ui.set_claude_mode("headless".into());
    assert_eq!(ui.get_claude_mode(), "headless");
    ui.set_git_user_name("Test Bot".into());
    assert_eq!(ui.get_git_user_name(), "Test Bot");
    ui.set_git_user_email("test@example.com".into());
    assert_eq!(ui.get_git_user_email(), "test@example.com");
    ui.set_cpu_limit("4".into());
    assert_eq!(ui.get_cpu_limit(), "4");
    ui.set_memory_limit("8Gi".into());
    assert_eq!(ui.get_memory_limit(), "8Gi");
    ui.set_terraform_dir("custom-tf".into());
    assert_eq!(ui.get_terraform_dir(), "custom-tf");
    ui.set_helm_chart_dir("custom-helm".into());
    assert_eq!(ui.get_helm_chart_dir(), "custom-helm");

    // -- set/get all-selected --
    ui.set_all_selected(true);
    assert!(ui.get_all_selected());
    ui.set_all_selected(false);
    assert!(!ui.get_all_selected());

    // -- set/get claude-found/claude-version --
    ui.set_claude_found(true);
    assert!(ui.get_claude_found());
    ui.set_claude_version("1.0.5".into());
    assert_eq!(ui.get_claude_version(), "1.0.5");

    // -- set/get active-launch-tab --
    ui.set_active_launch_tab(2);
    assert_eq!(ui.get_active_launch_tab(), 2);
    ui.set_active_launch_tab(0);

    // -- set/get docker-status --
    ui.set_docker_status("Running".into());
    assert_eq!(ui.get_docker_status(), "Running");
    ui.set_docker_status("Stopped".into());
    assert_eq!(ui.get_docker_status(), "Stopped");

    // -- set/get recovery-hint (CLU-29) --
    assert_eq!(ui.get_recovery_hint(), "", "recovery-hint default should be empty");
    ui.set_recovery_hint("Manual fix: Run 'k3d cluster delete'".into());
    assert_eq!(ui.get_recovery_hint(), "Manual fix: Run 'k3d cluster delete'");
    ui.set_recovery_hint("".into());
    assert_eq!(ui.get_recovery_hint(), "", "recovery-hint should be clearable");

    // -- set/get wsl-status (CLU-4) --
    assert_eq!(ui.get_wsl_status(), "Unknown", "wsl-status default should be Unknown");
    ui.set_wsl_status("Healthy".into());
    assert_eq!(ui.get_wsl_status(), "Healthy");
    ui.set_wsl_status("Unhealthy".into());
    assert_eq!(ui.get_wsl_status(), "Unhealthy");

    // -- set/get containers-status --
    ui.set_containers_status("3 running".into());
    assert_eq!(ui.get_containers_status(), "3 running");
    ui.set_containers_status("None".into());
    assert_eq!(ui.get_containers_status(), "None");

    // -- set projects model --
    let entries = vec![
        ProjectEntry {
            name: "frontend".into(),
            path: "/home/user/frontend".into(),
            selected: false,
            base_image_index: 0,
            has_custom_dockerfile: false,
            deployed: false,
            pod_failed: false,
            ambiguous: false,
        },
        ProjectEntry {
            name: "backend".into(),
            path: "/home/user/backend".into(),
            selected: true,
            base_image_index: 2,
            has_custom_dockerfile: true,
            deployed: false,
            pod_failed: false,
            ambiguous: false,
        },
    ];
    let model = std::rc::Rc::new(slint::VecModel::from(entries));
    ui.set_projects(model.into());

    let projects = ui.get_projects();
    assert_eq!(projects.row_count(), 2);
    assert_eq!(projects.row_data(0).unwrap().name, "frontend");
    assert!(!projects.row_data(0).unwrap().selected);
    assert_eq!(projects.row_data(0).unwrap().base_image_index, 0);
    assert_eq!(projects.row_data(1).unwrap().name, "backend");
    assert!(projects.row_data(1).unwrap().selected);
    assert_eq!(projects.row_data(1).unwrap().base_image_index, 2);
    assert!(projects.row_data(1).unwrap().has_custom_dockerfile);

    // -- set launch-tabs model --
    let tabs = vec![
        LaunchTab {
            name: "Summary".into(),
            status: "Building".into(),
            log_text: "Starting build...\n".into(),
        },
        LaunchTab {
            name: "my-project".into(),
            status: "Pending".into(),
            log_text: "".into(),
        },
    ];
    let tab_model = std::rc::Rc::new(slint::VecModel::from(tabs));
    ui.set_launch_tabs(tab_model.into());

    let launch_tabs = ui.get_launch_tabs();
    assert_eq!(launch_tabs.row_count(), 2);
    assert_eq!(launch_tabs.row_data(0).unwrap().name, "Summary");
    assert_eq!(launch_tabs.row_data(0).unwrap().status, "Building");
    assert_eq!(launch_tabs.row_data(1).unwrap().name, "my-project");
    assert_eq!(launch_tabs.row_data(1).unwrap().status, "Pending");

    // -- update tab via set_row_data --
    let mut tab0 = launch_tabs.row_data(0).unwrap();
    tab0.status = "Done".into();
    tab0.log_text = "All done!\n".into();
    launch_tabs.set_row_data(0, tab0);
    assert_eq!(launch_tabs.row_data(0).unwrap().status, "Done");
    assert_eq!(launch_tabs.row_data(0).unwrap().log_text, "All done!\n");

    // -- set pods model --
    let pod_entries = vec![
        PodEntry {
            name: "claude-frontend-abc".into(),
            project: "frontend".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 0,
            age: "2026-03-02T10:00:00Z".into(),
            warnings: "".into(),
            exposed: false,
            container_port: 0,
            selected: false,
            remote_control: false,
        },
        PodEntry {
            name: "claude-backend-def".into(),
            project: "backend".into(),
            phase: "Pending".into(),
            ready: false,
            restart_count: 5,
            age: "2026-03-02T09:30:00Z".into(),
            warnings: "FailedMount: path not found".into(),
            exposed: false,
            container_port: 0,
            selected: false,
            remote_control: false,
        },
    ];
    let pod_model = std::rc::Rc::new(slint::VecModel::from(pod_entries));
    ui.set_pods(pod_model.into());

    let pods = ui.get_pods();
    assert_eq!(pods.row_count(), 2);
    assert_eq!(pods.row_data(0).unwrap().project, "frontend");
    assert_eq!(pods.row_data(0).unwrap().phase, "Running");
    assert!(pods.row_data(0).unwrap().ready);
    assert_eq!(pods.row_data(0).unwrap().restart_count, 0);
    assert_eq!(pods.row_data(1).unwrap().project, "backend");
    assert_eq!(pods.row_data(1).unwrap().phase, "Pending");
    assert!(!pods.row_data(1).unwrap().ready);
    assert_eq!(pods.row_data(1).unwrap().restart_count, 5);

    // -- deps state defaults --
    assert!(ui.get_all_deps_met());
    assert!(!ui.get_k3s_found());
    assert!(!ui.get_terraform_found());
    assert!(!ui.get_helm_found());
    assert!(!ui.get_docker_found());
    assert_eq!(ui.get_k3s_version(), "");
    assert_eq!(ui.get_terraform_version(), "");
    assert_eq!(ui.get_helm_version(), "");
    assert_eq!(ui.get_docker_version(), "");
    assert_eq!(ui.get_install_log(), "");
    assert!(!ui.get_is_installing());

    // -- set/get deps state --
    ui.set_all_deps_met(false);
    assert!(!ui.get_all_deps_met());
    ui.set_k3s_found(true);
    assert!(ui.get_k3s_found());
    ui.set_k3s_version("v1.28.5+k3s1".into());
    assert_eq!(ui.get_k3s_version(), "v1.28.5+k3s1");
    ui.set_install_log("Installing terraform...".into());
    assert_eq!(ui.get_install_log(), "Installing terraform...");
    ui.set_is_installing(true);
    assert!(ui.get_is_installing());
    // Reset for remainder of tests
    ui.set_all_deps_met(true);
    ui.set_is_installing(false);

    // -- callback wiring (no panic) --
    ui.on_cluster_deploy(|| {});
    ui.on_terraform_destroy(|| {});
    ui.on_browse_folder(|| {});
    ui.on_refresh_projects(|| {});
    ui.on_launch_selected(|| {});
    ui.on_stop_selected(|| {});
    ui.on_cancel_launch(|| {});
    ui.on_toggle_select_all(|| {});
    ui.on_project_toggled(|_idx, _checked| {});
    ui.on_project_image_changed(|_idx, _img| {});
    ui.on_refresh_pods(|| {});
    ui.on_delete_pod(|_idx| {});
    ui.on_view_logs(|_idx| {});
    ui.on_terraform_plan(|| {});
    ui.on_helm_status(|| {});
    ui.on_save_settings(|| {});
    ui.on_exec_claude(|_idx| {});
    ui.on_shell_pod(|_idx| {});
    ui.on_install_missing(|| {});
    ui.on_continue_app(|| {});

    // -- callback invocation with counters --
    // Note: re-registering callbacks replaces the previous ones

    // cluster_deploy
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_cluster_deploy(move || { c.set(c.get() + 1); });
    ui.invoke_cluster_deploy();
    assert_eq!(counter.get(), 1);
    ui.invoke_cluster_deploy();
    assert_eq!(counter.get(), 2);

    // terraform_destroy
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_terraform_destroy(move || { c.set(c.get() + 1); });
    ui.invoke_terraform_destroy();
    assert_eq!(counter.get(), 1);

    // terraform_plan
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_terraform_plan(move || { c.set(c.get() + 1); });
    ui.invoke_terraform_plan();
    assert_eq!(counter.get(), 1);

    // helm_status
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_helm_status(move || { c.set(c.get() + 1); });
    ui.invoke_helm_status();
    assert_eq!(counter.get(), 1);

    // browse_folder
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_browse_folder(move || { c.set(c.get() + 1); });
    ui.invoke_browse_folder();
    assert_eq!(counter.get(), 1);

    // refresh_projects
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_refresh_projects(move || { c.set(c.get() + 1); });
    ui.invoke_refresh_projects();
    assert_eq!(counter.get(), 1);

    // launch_selected
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_launch_selected(move || { c.set(c.get() + 1); });
    ui.invoke_launch_selected();
    assert_eq!(counter.get(), 1);

    // stop_selected
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_stop_selected(move || { c.set(c.get() + 1); });
    ui.invoke_stop_selected();
    assert_eq!(counter.get(), 1);

    // cancel_launch
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_cancel_launch(move || { c.set(c.get() + 1); });
    ui.invoke_cancel_launch();
    assert_eq!(counter.get(), 1);

    // toggle_select_all
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_toggle_select_all(move || { c.set(c.get() + 1); });
    ui.invoke_toggle_select_all();
    assert_eq!(counter.get(), 1);

    // refresh_pods
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_refresh_pods(move || { c.set(c.get() + 1); });
    ui.invoke_refresh_pods();
    assert_eq!(counter.get(), 1);

    // save_settings
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_save_settings(move || { c.set(c.get() + 1); });
    ui.invoke_save_settings();
    assert_eq!(counter.get(), 1);

    // -- callbacks with arguments --

    // project_toggled
    let received_idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let received_checked = std::rc::Rc::new(std::cell::Cell::new(false));
    let i = received_idx.clone();
    let ch = received_checked.clone();
    ui.on_project_toggled(move |idx, checked| { i.set(idx); ch.set(checked); });
    ui.invoke_project_toggled(3, true);
    assert_eq!(received_idx.get(), 3);
    assert!(received_checked.get());

    // project_image_changed
    let received_idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let received_img = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let i = received_idx.clone();
    let im = received_img.clone();
    ui.on_project_image_changed(move |idx, img| { i.set(idx); im.set(img); });
    ui.invoke_project_image_changed(2, 5);
    assert_eq!(received_idx.get(), 2);
    assert_eq!(received_img.get(), 5);

    // delete_pod
    let received_idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let i = received_idx.clone();
    ui.on_delete_pod(move |idx| { i.set(idx); });
    ui.invoke_delete_pod(7);
    assert_eq!(received_idx.get(), 7);

    // view_logs
    let received_idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let i = received_idx.clone();
    ui.on_view_logs(move |idx| { i.set(idx); });
    ui.invoke_view_logs(4);
    assert_eq!(received_idx.get(), 4);

    // exec_claude
    let received_idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let i = received_idx.clone();
    ui.on_exec_claude(move |idx| { i.set(idx); });
    ui.invoke_exec_claude(2);
    assert_eq!(received_idx.get(), 2);

    // shell_pod
    let received_idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let i = received_idx.clone();
    ui.on_shell_pod(move |idx| { i.set(idx); });
    ui.invoke_shell_pod(5);
    assert_eq!(received_idx.get(), 5);

    // install_missing
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_install_missing(move || { c.set(c.get() + 1); });
    ui.invoke_install_missing();
    assert_eq!(counter.get(), 1);

    // continue_app
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_continue_app(move || { c.set(c.get() + 1); });
    ui.invoke_continue_app();
    assert_eq!(counter.get(), 1);

    // -- new bulk action callbacks (no panic) --
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_redeploy_selected(move || { c.set(c.get() + 1); });
    ui.invoke_redeploy_selected();
    assert_eq!(counter.get(), 1);

    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_expose_selected(move || { c.set(c.get() + 1); });
    ui.invoke_expose_selected();
    assert_eq!(counter.get(), 1);

    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_unexpose_selected(move || { c.set(c.get() + 1); });
    ui.invoke_unexpose_selected();
    assert_eq!(counter.get(), 1);

    // -- selected_pod_names property --
    ui.set_selected_pod_names("frontend, backend".into());
    assert_eq!(ui.get_selected_pod_names(), "frontend, backend");
    ui.set_selected_pod_names("".into());
    assert_eq!(ui.get_selected_pod_names(), "");

    // -- selected_pod_count and all_pods_selected --
    ui.set_selected_pod_count(3);
    assert_eq!(ui.get_selected_pod_count(), 3);
    ui.set_all_pods_selected(true);
    assert!(ui.get_all_pods_selected());
    ui.set_all_pods_selected(false);
    assert!(!ui.get_all_pods_selected());

    // -- PodEntry selected field updates through model --
    let pod_entries = vec![
        PodEntry {
            name: "pod-1".into(),
            project: "proj-a".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 0,
            age: "5m".into(),
            warnings: "".into(),
            exposed: false,
            container_port: 3000,
            selected: false,
            remote_control: false,
        },
        PodEntry {
            name: "pod-2".into(),
            project: "proj-b".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 0,
            age: "10m".into(),
            warnings: "".into(),
            exposed: true,
            container_port: 8080,
            selected: true,
            remote_control: false,
        },
    ];
    let pod_model = std::rc::Rc::new(slint::VecModel::from(pod_entries));
    ui.set_pods(pod_model.into());

    let pods = ui.get_pods();
    assert!(!pods.row_data(0).unwrap().selected, "pod-1 should start unselected");
    assert!(pods.row_data(1).unwrap().selected, "pod-2 should start selected");
    assert!(pods.row_data(1).unwrap().exposed, "pod-2 should be exposed");
    assert_eq!(pods.row_data(1).unwrap().container_port, 8080);

    // Toggle selection on pod-1
    let mut p0 = pods.row_data(0).unwrap();
    p0.selected = true;
    pods.set_row_data(0, p0);
    assert!(pods.row_data(0).unwrap().selected, "pod-1 should now be selected");

    // -- pod_toggled callback with arguments --
    let received_idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let received_checked = std::rc::Rc::new(std::cell::Cell::new(false));
    let i = received_idx.clone();
    let ch = received_checked.clone();
    ui.on_pod_toggled(move |idx, checked| { i.set(idx); ch.set(checked); });
    ui.invoke_pod_toggled(1, true);
    assert_eq!(received_idx.get(), 1);
    assert!(received_checked.get());

    // -- toggle_select_all_pods --
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_toggle_select_all_pods(move |_checked| { c.set(c.get() + 1); });
    ui.invoke_toggle_select_all_pods(true);
    assert_eq!(counter.get(), 1);

    // -- delete_selected_pods --
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_delete_selected_pods(move || { c.set(c.get() + 1); });
    ui.invoke_delete_selected_pods();
    assert_eq!(counter.get(), 1);

    // -- toggle_network --
    let received_idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let i = received_idx.clone();
    ui.on_toggle_network(move |idx| { i.set(idx); });
    ui.invoke_toggle_network(3);
    assert_eq!(received_idx.get(), 3);

    // -- pod-log property --
    ui.set_pod_log("Container started\nListening on :8080".into());
    assert_eq!(ui.get_pod_log(), "Container started\nListening on :8080");
    ui.set_pod_log("".into());
    assert_eq!(ui.get_pod_log(), "");

    // -- node_status, helm_release_status --
    ui.set_node_status("1 Ready".into());
    assert_eq!(ui.get_node_status(), "1 Ready");
    ui.set_helm_release_status("deployed".into());
    assert_eq!(ui.get_helm_release_status(), "deployed");

    // -- memory_usage_text, cluster_memory_info --
    ui.set_memory_usage_text("2.1 GB of 8.0 GB".into());
    assert_eq!(ui.get_memory_usage_text(), "2.1 GB of 8.0 GB");
    ui.set_cluster_memory_info("4.0 GB of 16.0 GB".into());
    assert_eq!(ui.get_cluster_memory_info(), "4.0 GB of 16.0 GB");

    // -- cluster_memory_percent --
    ui.set_cluster_memory_percent("50".into());
    assert_eq!(ui.get_cluster_memory_percent(), "50");

    // -- launch_steps model --
    let steps = vec![
        LaunchStep {
            label: "Build Images".into(),
            status: "done".into(),
            message: "Built 3 images".into(),
        },
        LaunchStep {
            label: "Import to Cluster".into(),
            status: "running".into(),
            message: "Importing...".into(),
        },
        LaunchStep {
            label: "Helm Deploy".into(),
            status: "pending".into(),
            message: "".into(),
        },
    ];
    let step_model = std::rc::Rc::new(slint::VecModel::from(steps));
    ui.set_launch_steps(step_model.into());

    let launch_steps = ui.get_launch_steps();
    assert_eq!(launch_steps.row_count(), 3);
    assert_eq!(launch_steps.row_data(0).unwrap().label, "Build Images");
    assert_eq!(launch_steps.row_data(0).unwrap().status, "done");
    assert_eq!(launch_steps.row_data(1).unwrap().status, "running");
    assert_eq!(launch_steps.row_data(2).unwrap().status, "pending");

    // -- update step status --
    let mut step1 = launch_steps.row_data(1).unwrap();
    step1.status = "done".into();
    step1.message = "Imported 3 images".into();
    launch_steps.set_row_data(1, step1);
    assert_eq!(launch_steps.row_data(1).unwrap().status, "done");

    // -- shell_pod callback --
    let received_idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let i = received_idx.clone();
    ui.on_shell_pod(move |idx| { i.set(idx); });
    ui.invoke_shell_pod(3);
    assert_eq!(received_idx.get(), 3);

    // -- PodEntry remote_control field --
    let rc_pods = vec![
        PodEntry {
            name: "rc-pod-1".into(),
            project: "proj".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 0,
            age: "5m".into(),
            warnings: "".into(),
            exposed: false,
            container_port: 0,
            selected: false,
            remote_control: false,
        },
        PodEntry {
            name: "rc-pod-2".into(),
            project: "proj".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 0,
            age: "5m".into(),
            warnings: "".into(),
            exposed: false,
            container_port: 0,
            selected: false,
            remote_control: true,
        },
    ];
    let rc_model = std::rc::Rc::new(slint::VecModel::from(rc_pods));
    ui.set_pods(rc_model.into());
    let rc_result = ui.get_pods();
    assert!(!rc_result.row_data(0).unwrap().remote_control, "pod-1 remote_control should be false");
    assert!(rc_result.row_data(1).unwrap().remote_control, "pod-2 remote_control should be true");

    // Toggle remote_control via model update
    let mut rc_p = rc_result.row_data(0).unwrap();
    rc_p.remote_control = true;
    rc_result.set_row_data(0, rc_p);
    assert!(rc_result.row_data(0).unwrap().remote_control, "pod-1 remote_control should now be true");

    // -- toggle_select_all_pods receives bool --
    let received_checked = std::rc::Rc::new(std::cell::Cell::new(false));
    let ch = received_checked.clone();
    ui.on_toggle_select_all_pods(move |checked| { ch.set(checked); });
    ui.invoke_toggle_select_all_pods(true);
    assert!(received_checked.get(), "should receive true");
    ui.invoke_toggle_select_all_pods(false);
    assert!(!received_checked.get(), "should receive false");

    // -- active_page navigation --
    ui.set_active_page(0);
    assert_eq!(ui.get_active_page(), 0);
    ui.set_active_page(2);
    assert_eq!(ui.get_active_page(), 2);
    ui.set_active_page(3);
    assert_eq!(ui.get_active_page(), 3);
}
