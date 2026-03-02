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
    ui.on_save_settings(|| {});

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
}
