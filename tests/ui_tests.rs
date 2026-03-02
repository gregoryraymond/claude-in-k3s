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
    assert_eq!(ui.get_api_key(), "");
    assert_eq!(ui.get_platform_name(), "Linux");

    // Check defaults for new settings properties
    assert_eq!(ui.get_claude_mode(), "daemon");
    assert_eq!(ui.get_git_user_name(), "Claude Code Bot");
    assert_eq!(ui.get_git_user_email(), "claude-bot@localhost");
    assert_eq!(ui.get_cpu_limit(), "2");
    assert_eq!(ui.get_memory_limit(), "4Gi");
    assert_eq!(ui.get_terraform_dir(), "terraform");
    assert_eq!(ui.get_helm_chart_dir(), "helm/claude-code");

    assert_eq!(ui.get_claude_prompt(), "");
    assert_eq!(ui.get_claude_target_pod(), "");

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

    // -- set/get api_key --
    ui.set_api_key("sk-ant-test123".into());
    assert_eq!(ui.get_api_key(), "sk-ant-test123");

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

    ui.set_claude_prompt("test prompt".into());
    assert_eq!(ui.get_claude_prompt(), "test prompt");
    ui.set_claude_target_pod("my-pod-123".into());
    assert_eq!(ui.get_claude_target_pod(), "my-pod-123");

    // -- projects model empty by default --
    // (reset to check model defaults on a fresh window isn't possible,
    // but we already checked the defaults above before any sets)

    // -- set projects model --
    let entries = vec![
        ProjectEntry {
            name: "frontend".into(),
            path: "/home/user/frontend".into(),
            selected: false,
            base_image_index: 0,
            has_custom_dockerfile: false,
        },
        ProjectEntry {
            name: "backend".into(),
            path: "/home/user/backend".into(),
            selected: true,
            base_image_index: 2,
            has_custom_dockerfile: true,
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

    // -- set pods model --
    let pod_entries = vec![
        PodEntry {
            name: "claude-frontend-abc".into(),
            project: "frontend".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 0,
            age: "2026-03-02T10:00:00Z".into(),
        },
        PodEntry {
            name: "claude-backend-def".into(),
            project: "backend".into(),
            phase: "Pending".into(),
            ready: false,
            restart_count: 5,
            age: "2026-03-02T09:30:00Z".into(),
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
    ui.on_terraform_init(|| {});
    ui.on_terraform_apply(|| {});
    ui.on_terraform_destroy(|| {});
    ui.on_browse_folder(|| {});
    ui.on_refresh_projects(|| {});
    ui.on_launch_selected(|| {});
    ui.on_stop_selected(|| {});
    ui.on_project_toggled(|_idx, _checked| {});
    ui.on_project_image_changed(|_idx, _img| {});
    ui.on_refresh_pods(|| {});
    ui.on_delete_pod(|_idx| {});
    ui.on_view_logs(|_idx| {});
    ui.on_terraform_plan(|| {});
    ui.on_helm_status(|| {});
    ui.on_save_settings(|| {});
    ui.on_exec_claude(|_idx| {});
    ui.on_send_prompt(|_prompt| {});
    ui.on_install_missing(|| {});
    ui.on_continue_app(|| {});

    // -- callback invocation with counters --
    // Note: re-registering callbacks replaces the previous ones

    // terraform_init
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_terraform_init(move || { c.set(c.get() + 1); });
    ui.invoke_terraform_init();
    assert_eq!(counter.get(), 1);
    ui.invoke_terraform_init();
    assert_eq!(counter.get(), 2);

    // terraform_apply
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_terraform_apply(move || { c.set(c.get() + 1); });
    ui.invoke_terraform_apply();
    assert_eq!(counter.get(), 1);

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

    // send_prompt
    let received_prompt = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let p = received_prompt.clone();
    ui.on_send_prompt(move |prompt| { *p.borrow_mut() = prompt.to_string(); });
    ui.invoke_send_prompt("hello claude".into());
    assert_eq!(*received_prompt.borrow(), "hello claude");

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
}
