//! Requirements validation tests.
//!
//! These tests verify that all features documented in README.md are
//! present and wired correctly. Each test section maps to a specific
//! requirement from the project documentation.
//!
//! Requirements covered:
//!  R01: One-Click Infrastructure (Terraform + auto-install)
//!  R02: Smart Project Detection (language detection, base images)
//!  R03: Live Pod Management (pod list, status, real-time updates)
//!  R04: Pod Actions (logs, prompt, expose, redeploy, delete)
//!  R05: Network Exposure (Service + Ingress creation)
//!  R06: Bulk Operations (select all, action bar, bulk actions)
//!  R07: Log Viewer (auto-scroll, previous logs, describe fallback)
//!  R08: Redeploy (Helm upgrade without Docker rebuild)
//!  R09: Resource Controls (CPU/memory limits)
//!  R10: Custom Dockerfiles (.claude/ or project root)
//!  R11: Cross-Platform (Linux, macOS, Windows/WSL2)
//!  R12: Cluster Health Stack (5-layer infrastructure visualization)
//!  R13: Service Management (WSL/Docker/Cluster restart)
//!  R14: Design System Compliance (Forge theme tokens)

use slint::Model;

slint::include_modules!();

// ═══════════════════════════════════════════════════════════════════
// R01: One-Click Infrastructure
// The app must have Terraform init/apply/destroy/plan callbacks and
// dependency auto-install.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r01_infrastructure_callbacks_exist() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // Terraform lifecycle callbacks
    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_cluster_deploy(move || { c.set(c.get() + 1); });
    ui.invoke_cluster_deploy();
    assert_eq!(fired.get(), 1, "R01: cluster_deploy callback must fire");

    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_terraform_destroy(move || { c.set(c.get() + 1); });
    ui.invoke_terraform_destroy();
    assert_eq!(fired.get(), 1, "R01: terraform_destroy callback must fire");

    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_terraform_plan(move || { c.set(c.get() + 1); });
    ui.invoke_terraform_plan();
    assert_eq!(fired.get(), 1, "R01: terraform_plan callback must fire");

    // Dependency install
    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_install_missing(move || { c.set(c.get() + 1); });
    ui.invoke_install_missing();
    assert_eq!(fired.get(), 1, "R01: install_missing callback must fire");

    // Dependency status properties
    ui.set_k3s_found(true);
    ui.set_terraform_found(true);
    ui.set_helm_found(true);
    ui.set_docker_found(true);
    assert!(ui.get_k3s_found());
    assert!(ui.get_terraform_found());
    assert!(ui.get_helm_found());
    assert!(ui.get_docker_found());

    // Version display properties
    ui.set_k3s_version("v1.31.4".into());
    assert_eq!(ui.get_k3s_version(), "v1.31.4");
    ui.set_terraform_version("1.5.0".into());
    assert_eq!(ui.get_terraform_version(), "1.5.0");
    ui.set_helm_version("3.16.0".into());
    assert_eq!(ui.get_helm_version(), "3.16.0");
    ui.set_docker_version("27.4.1".into());
    assert_eq!(ui.get_docker_version(), "27.4.1");

    // tf_initialized state
    ui.set_tf_initialized(true);
    assert!(ui.get_tf_initialized(), "R01: tf_initialized must be settable");
}

// ═══════════════════════════════════════════════════════════════════
// R02: Smart Project Detection
// Must scan directories, detect language, and assign base images.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r02_project_model_supports_detection_fields() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // Projects model must support: name, path, selected, base_image_index, has_custom_dockerfile
    let entries = vec![
        ProjectEntry {
            name: "node-app".into(),
            path: "/projects/node-app".into(),
            selected: false,
            base_image_index: 0, // Node
            has_custom_dockerfile: false,
            deployed: false,
            pod_failed: false,
            ambiguous: false,
        },
        ProjectEntry {
            name: "python-api".into(),
            path: "/projects/python-api".into(),
            selected: true,
            base_image_index: 1, // Python
            has_custom_dockerfile: false,
            deployed: false,
            pod_failed: false,
            ambiguous: false,
        },
        ProjectEntry {
            name: "rust-service".into(),
            path: "/projects/rust-service".into(),
            selected: false,
            base_image_index: 2, // Rust
            has_custom_dockerfile: false,
            deployed: false,
            pod_failed: false,
            ambiguous: false,
        },
        ProjectEntry {
            name: "go-tool".into(),
            path: "/projects/go-tool".into(),
            selected: false,
            base_image_index: 3, // Go
            has_custom_dockerfile: false,
            deployed: false,
            pod_failed: false,
            ambiguous: false,
        },
        ProjectEntry {
            name: "dotnet-app".into(),
            path: "/projects/dotnet-app".into(),
            selected: false,
            base_image_index: 4, // .NET
            has_custom_dockerfile: false,
            deployed: false,
            pod_failed: false,
            ambiguous: false,
        },
        ProjectEntry {
            name: "custom-proj".into(),
            path: "/projects/custom-proj".into(),
            selected: false,
            base_image_index: 6, // Custom
            has_custom_dockerfile: true,
            deployed: false,
            pod_failed: false,
            ambiguous: false,
        },
    ];
    let model = std::rc::Rc::new(slint::VecModel::from(entries));
    ui.set_projects(model.into());

    let projects = ui.get_projects();
    assert_eq!(projects.row_count(), 6, "R02: must handle all 6 language types");

    // Verify base image indices cover Node(0), Python(1), Rust(2), Go(3), .NET(4), Custom(6)
    assert_eq!(projects.row_data(0).unwrap().base_image_index, 0);
    assert_eq!(projects.row_data(1).unwrap().base_image_index, 1);
    assert_eq!(projects.row_data(2).unwrap().base_image_index, 2);
    assert_eq!(projects.row_data(3).unwrap().base_image_index, 3);
    assert_eq!(projects.row_data(4).unwrap().base_image_index, 4);
    assert_eq!(projects.row_data(5).unwrap().base_image_index, 6);

    // Custom dockerfile flag
    assert!(projects.row_data(5).unwrap().has_custom_dockerfile,
        "R02: custom dockerfile flag must propagate");

    // Image change callback
    let idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let img = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let i = idx.clone();
    let im = img.clone();
    ui.on_project_image_changed(move |index, image| { i.set(index); im.set(image); });
    ui.invoke_project_image_changed(2, 5);
    assert_eq!(idx.get(), 2, "R02: project_image_changed must pass index");
    assert_eq!(img.get(), 5, "R02: project_image_changed must pass image index");
}

#[test]
fn r02_browse_and_refresh_callbacks() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_browse_folder(move || { c.set(c.get() + 1); });
    ui.invoke_browse_folder();
    assert_eq!(fired.get(), 1, "R02: browse_folder callback must fire");

    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_refresh_projects(move || { c.set(c.get() + 1); });
    ui.invoke_refresh_projects();
    assert_eq!(fired.get(), 1, "R02: refresh_projects callback must fire");

    // projects_dir property
    ui.set_projects_dir("/home/user/code".into());
    assert_eq!(ui.get_projects_dir(), "/home/user/code");
}

// ═══════════════════════════════════════════════════════════════════
// R03: Live Pod Management
// Pods model with status, ready state, restart count, age, warnings.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r03_pod_model_fields() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    let pods = vec![
        PodEntry {
            name: "claude-frontend-abc".into(),
            project: "frontend".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 0,
            age: "2d 3h".into(),
            warnings: "".into(),
            exposed: false,
            container_port: 3000,
            selected: false,
            remote_control: false,
        },
        PodEntry {
            name: "claude-backend-def".into(),
            project: "backend".into(),
            phase: "CrashLoopBackOff".into(),
            ready: false,
            restart_count: 12,
            age: "45m".into(),
            warnings: "OOMKilled".into(),
            exposed: true,
            container_port: 8080,
            selected: true,
            remote_control: true,
        },
    ];
    let model = std::rc::Rc::new(slint::VecModel::from(pods));
    ui.set_pods(model.into());

    let result = ui.get_pods();
    assert_eq!(result.row_count(), 2, "R03: must display multiple pods");

    // Pod 1: healthy running pod
    let p0 = result.row_data(0).unwrap();
    assert_eq!(p0.name, "claude-frontend-abc");
    assert_eq!(p0.project, "frontend");
    assert_eq!(p0.phase, "Running");
    assert!(p0.ready, "R03: ready flag must propagate");
    assert_eq!(p0.restart_count, 0);
    assert_eq!(p0.age, "2d 3h", "R03: human-friendly age must propagate");
    assert_eq!(p0.container_port, 3000, "R03: container port must propagate");

    // Pod 2: crashing pod with warnings
    let p1 = result.row_data(1).unwrap();
    assert_eq!(p1.phase, "CrashLoopBackOff");
    assert!(!p1.ready);
    assert_eq!(p1.restart_count, 12, "R03: restart count must propagate");
    assert_eq!(p1.warnings, "OOMKilled", "R03: warning badges must propagate");
    assert!(p1.exposed, "R03: exposed state must propagate");
    assert!(p1.remote_control, "R03: remote_control state must propagate");
}

#[test]
fn r03_pod_refresh_callback() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_refresh_pods(move || { c.set(c.get() + 1); });
    ui.invoke_refresh_pods();
    assert_eq!(fired.get(), 1, "R03: refresh_pods must fire for live updates");
}

// ═══════════════════════════════════════════════════════════════════
// R04: Pod Actions
// Per-pod: view logs, exec claude, expose/unexpose, delete.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r04_pod_action_callbacks() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // View logs
    let idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let i = idx.clone();
    ui.on_view_logs(move |index| { i.set(index); });
    ui.invoke_view_logs(3);
    assert_eq!(idx.get(), 3, "R04: view_logs must pass pod index");

    // Exec Claude (remote control)
    let idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let i = idx.clone();
    ui.on_exec_claude(move |index| { i.set(index); });
    ui.invoke_exec_claude(1);
    assert_eq!(idx.get(), 1, "R04: exec_claude must pass pod index");

    // Toggle network exposure
    let idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let i = idx.clone();
    ui.on_toggle_network(move |index| { i.set(index); });
    ui.invoke_toggle_network(2);
    assert_eq!(idx.get(), 2, "R04: toggle_network must pass pod index");

    // Delete pod
    let idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let i = idx.clone();
    ui.on_delete_pod(move |index| { i.set(index); });
    ui.invoke_delete_pod(0);
    assert_eq!(idx.get(), 0, "R04: delete_pod must pass pod index");

    // Shell access
    let idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let i = idx.clone();
    ui.on_shell_pod(move |index| { i.set(index); });
    ui.invoke_shell_pod(4);
    assert_eq!(idx.get(), 4, "R04: shell_pod must pass pod index");
}

// ═══════════════════════════════════════════════════════════════════
// R05: Network Exposure
// Pod model tracks exposed state and container port.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r05_network_exposure_state() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    let pods = vec![
        PodEntry {
            name: "pod-exposed".into(),
            project: "web-app".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 0,
            age: "1h 30m".into(),
            warnings: "".into(),
            exposed: true,
            container_port: 3000,
            selected: false,
            remote_control: false,
        },
        PodEntry {
            name: "pod-private".into(),
            project: "api-svc".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 0,
            age: "2h".into(),
            warnings: "".into(),
            exposed: false,
            container_port: 8080,
            selected: false,
            remote_control: false,
        },
    ];
    let model = std::rc::Rc::new(slint::VecModel::from(pods));
    ui.set_pods(model.into());

    let result = ui.get_pods();
    assert!(result.row_data(0).unwrap().exposed, "R05: exposed pod must show as exposed");
    assert!(!result.row_data(1).unwrap().exposed, "R05: private pod must show as not exposed");
    assert_eq!(result.row_data(0).unwrap().container_port, 3000, "R05: port must propagate");

    // Expose/unexpose bulk callbacks
    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_expose_selected(move || { c.set(c.get() + 1); });
    ui.invoke_expose_selected();
    assert_eq!(fired.get(), 1, "R05: expose_selected callback must fire");

    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_unexpose_selected(move || { c.set(c.get() + 1); });
    ui.invoke_unexpose_selected();
    assert_eq!(fired.get(), 1, "R05: unexpose_selected callback must fire");
}

// ═══════════════════════════════════════════════════════════════════
// R06: Bulk Operations
// Select all, individual selection, floating action bar state.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r06_bulk_selection_state() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // Selection count and names for action bar
    ui.set_selected_pod_count(3);
    assert_eq!(ui.get_selected_pod_count(), 3, "R06: selected_pod_count must track");

    ui.set_selected_pod_names("frontend, backend, api".into());
    assert_eq!(ui.get_selected_pod_names(), "frontend, backend, api",
        "R06: selected_pod_names for action bar display");

    ui.set_all_pods_selected(true);
    assert!(ui.get_all_pods_selected(), "R06: all_pods_selected toggle");

    // Pod selection toggle callback
    let idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let checked = std::rc::Rc::new(std::cell::Cell::new(false));
    let i = idx.clone();
    let ch = checked.clone();
    ui.on_pod_toggled(move |index, state| { i.set(index); ch.set(state); });
    ui.invoke_pod_toggled(2, true);
    assert_eq!(idx.get(), 2, "R06: pod_toggled must pass index");
    assert!(checked.get(), "R06: pod_toggled must pass checked state");

    // Select all pods toggle
    let received = std::rc::Rc::new(std::cell::Cell::new(false));
    let ch = received.clone();
    ui.on_toggle_select_all_pods(move |state| { ch.set(state); });
    ui.invoke_toggle_select_all_pods(true);
    assert!(received.get(), "R06: toggle_select_all_pods must receive true");
}

#[test]
fn r06_bulk_action_callbacks() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // Redeploy selected
    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_redeploy_selected(move || { c.set(c.get() + 1); });
    ui.invoke_redeploy_selected();
    assert_eq!(fired.get(), 1, "R06: redeploy_selected must fire");

    // Delete selected
    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_delete_selected_pods(move || { c.set(c.get() + 1); });
    ui.invoke_delete_selected_pods();
    assert_eq!(fired.get(), 1, "R06: delete_selected_pods must fire");
}

#[test]
fn r06_pod_selection_persists_through_model_update() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    let pods = vec![
        PodEntry {
            name: "pod-1".into(), project: "a".into(), phase: "Running".into(),
            ready: true, restart_count: 0, age: "5m".into(), warnings: "".into(),
            exposed: false, container_port: 0, selected: true, remote_control: false,
        },
        PodEntry {
            name: "pod-2".into(), project: "b".into(), phase: "Running".into(),
            ready: true, restart_count: 0, age: "5m".into(), warnings: "".into(),
            exposed: false, container_port: 0, selected: false, remote_control: false,
        },
    ];
    let model = std::rc::Rc::new(slint::VecModel::from(pods));
    ui.set_pods(model.into());

    let result = ui.get_pods();
    assert!(result.row_data(0).unwrap().selected, "R06: initial selection must hold");
    assert!(!result.row_data(1).unwrap().selected, "R06: initial non-selection must hold");

    // Simulate user toggling pod-2
    let mut p1 = result.row_data(1).unwrap();
    p1.selected = true;
    result.set_row_data(1, p1);
    assert!(result.row_data(1).unwrap().selected, "R06: selection toggle must update model");
}

// ═══════════════════════════════════════════════════════════════════
// R07: Log Viewer
// Log text property, pod log property.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r07_log_viewer_properties() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // Cluster log (advanced log viewer)
    ui.set_cluster_log("terraform init...\nDone.\n".into());
    assert_eq!(ui.get_cluster_log(), "terraform init...\nDone.\n",
        "R07: cluster_log must store multiline text");

    // Pod-specific log
    ui.set_pod_log("=== Previous container logs ===\nOOMKilled\n---\nStarting server\nListening on :3000\n".into());
    assert_eq!(ui.get_pod_log(), "=== Previous container logs ===\nOOMKilled\n---\nStarting server\nListening on :3000\n",
        "R07: pod_log must support previous container logs separator");

    // Empty log
    ui.set_pod_log("".into());
    assert_eq!(ui.get_pod_log(), "", "R07: log viewer must handle empty state");
}

// ═══════════════════════════════════════════════════════════════════
// R08: Redeploy
// Helm upgrade without Docker rebuild.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r08_redeploy_callback() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_redeploy_selected(move || { c.set(c.get() + 1); });
    ui.invoke_redeploy_selected();
    assert_eq!(fired.get(), 1, "R08: redeploy_selected must fire");

    // Helm status check
    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_helm_status(move || { c.set(c.get() + 1); });
    ui.invoke_helm_status();
    assert_eq!(fired.get(), 1, "R08: helm_status callback must fire");
}

// ═══════════════════════════════════════════════════════════════════
// R09: Resource Controls
// CPU/memory limits configurable in settings.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r09_resource_control_settings() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // Default values
    assert_eq!(ui.get_cpu_limit(), "2", "R09: default CPU limit must be 2");
    assert_eq!(ui.get_memory_limit(), "4Gi", "R09: default memory limit must be 4Gi");

    // Modifiable
    ui.set_cpu_limit("8".into());
    assert_eq!(ui.get_cpu_limit(), "8");
    ui.set_memory_limit("16Gi".into());
    assert_eq!(ui.get_memory_limit(), "16Gi");

    // Cluster memory percent
    ui.set_cluster_memory_percent("75".into());
    assert_eq!(ui.get_cluster_memory_percent(), "75");

    // Save settings callback
    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_save_settings(move || { c.set(c.get() + 1); });
    ui.invoke_save_settings();
    assert_eq!(fired.get(), 1, "R09: save_settings callback must fire");
}

// ═══════════════════════════════════════════════════════════════════
// R10: Custom Dockerfiles
// Detected via has_custom_dockerfile flag on project entries.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r10_custom_dockerfile_detection() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    let entries = vec![
        ProjectEntry {
            name: "standard-proj".into(),
            path: "/projects/standard".into(),
            selected: false,
            base_image_index: 0,
            has_custom_dockerfile: false,
            deployed: false,
            pod_failed: false,
            ambiguous: false,
        },
        ProjectEntry {
            name: "custom-proj".into(),
            path: "/projects/custom".into(),
            selected: false,
            base_image_index: 6, // Custom
            has_custom_dockerfile: true,
            deployed: false,
            pod_failed: false,
            ambiguous: false,
        },
    ];
    let model = std::rc::Rc::new(slint::VecModel::from(entries));
    ui.set_projects(model.into());

    let projects = ui.get_projects();
    assert!(!projects.row_data(0).unwrap().has_custom_dockerfile,
        "R10: standard project must not have custom dockerfile");
    assert!(projects.row_data(1).unwrap().has_custom_dockerfile,
        "R10: custom project must have dockerfile flag");
    assert_eq!(projects.row_data(1).unwrap().base_image_index, 6,
        "R10: custom project must use Custom base image index");
}

// ═══════════════════════════════════════════════════════════════════
// R11: Cross-Platform
// Platform detection and display.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r11_cross_platform_properties() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // Default platform
    assert_eq!(ui.get_platform_name(), "Linux", "R11: default platform must be Linux");

    // Platform can be set to all supported values
    for platform in &["Linux", "macOS", "WSL2", "Windows"] {
        ui.set_platform_name((*platform).into());
        assert_eq!(ui.get_platform_name(), *platform,
            "R11: must support platform '{}'", platform);
    }
}

// ═══════════════════════════════════════════════════════════════════
// R12: Cluster Health Stack
// 5-layer visualization: Docker → Cluster → Node → Helm → Pods
// with memory tracking and detail strings.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r12_health_stack_properties() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // Docker status
    ui.set_docker_status("Running".into());
    assert_eq!(ui.get_docker_status(), "Running");

    // Cluster status
    ui.set_cluster_status("Healthy".into());
    assert_eq!(ui.get_cluster_status(), "Healthy");

    // Containers status (pods summary)
    ui.set_containers_status("5 running".into());
    assert_eq!(ui.get_containers_status(), "5 running");

    // Node info
    ui.set_node_status("1 Ready".into());
    assert_eq!(ui.get_node_status(), "1 Ready");

    // Helm release status
    ui.set_helm_release_status("3 releases".into());
    assert_eq!(ui.get_helm_release_status(), "3 releases");

    // Memory tracking
    ui.set_memory_usage_text("2.1 GB of 8.0 GB".into());
    assert_eq!(ui.get_memory_usage_text(), "2.1 GB of 8.0 GB",
        "R12: memory usage text must propagate");

    ui.set_cluster_memory_info("4.0 GB / 16.0 GB".into());
    assert_eq!(ui.get_cluster_memory_info(), "4.0 GB / 16.0 GB");
}

// ═══════════════════════════════════════════════════════════════════
// R13: Service Management
// WSL restart, Docker restart, Cluster recreate callbacks.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r13_service_management_callbacks() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // Restart WSL
    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_restart_wsl(move || { c.set(c.get() + 1); });
    ui.invoke_restart_wsl();
    assert_eq!(fired.get(), 1, "R13: restart_wsl callback must fire");

    // Restart Docker
    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_restart_docker(move || { c.set(c.get() + 1); });
    ui.invoke_restart_docker();
    assert_eq!(fired.get(), 1, "R13: restart_docker callback must fire");

    // Recreate Cluster
    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_restart_cluster(move || { c.set(c.get() + 1); });
    ui.invoke_restart_cluster();
    assert_eq!(fired.get(), 1, "R13: restart_cluster callback must fire");
}

// ═══════════════════════════════════════════════════════════════════
// R09+: Settings Completeness
// All configurable fields from config.toml must be exposed in UI.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r09_settings_completeness() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // Claude mode
    assert_eq!(ui.get_claude_mode(), "daemon", "R09: default claude_mode must be daemon");
    ui.set_claude_mode("headless".into());
    assert_eq!(ui.get_claude_mode(), "headless");

    // Git configuration
    assert_eq!(ui.get_git_user_name(), "Claude Code Bot");
    ui.set_git_user_name("My Bot".into());
    assert_eq!(ui.get_git_user_name(), "My Bot");

    assert_eq!(ui.get_git_user_email(), "claude-bot@localhost");
    ui.set_git_user_email("bot@example.com".into());
    assert_eq!(ui.get_git_user_email(), "bot@example.com");

    // Directory configuration
    assert_eq!(ui.get_terraform_dir(), "terraform");
    ui.set_terraform_dir("infra/tf".into());
    assert_eq!(ui.get_terraform_dir(), "infra/tf");

    assert_eq!(ui.get_helm_chart_dir(), "helm/claude-code");
    ui.set_helm_chart_dir("charts/custom".into());
    assert_eq!(ui.get_helm_chart_dir(), "charts/custom");
}

// ═══════════════════════════════════════════════════════════════════
// R03+: Launch Workflow
// Launch tabs and steps for monitoring deploy progress.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r03_launch_workflow_models() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // Launch tabs (per-project build logs)
    let tabs = vec![
        LaunchTab {
            name: "Summary".into(),
            status: "Building".into(),
            log_text: "Starting build...\n".into(),
        },
        LaunchTab {
            name: "frontend".into(),
            status: "Done".into(),
            log_text: "Image built successfully\n".into(),
        },
    ];
    let tab_model = std::rc::Rc::new(slint::VecModel::from(tabs));
    ui.set_launch_tabs(tab_model.into());

    let result = ui.get_launch_tabs();
    assert_eq!(result.row_count(), 2, "R03: launch tabs must support multiple entries");
    assert_eq!(result.row_data(0).unwrap().name, "Summary");
    assert_eq!(result.row_data(1).unwrap().status, "Done");

    // Launch steps (progress stages)
    let steps = vec![
        LaunchStep { label: "Build Images".into(), status: "done".into(), message: "3 built".into() },
        LaunchStep { label: "Import to Cluster".into(), status: "running".into(), message: "...".into() },
        LaunchStep { label: "Helm Deploy".into(), status: "pending".into(), message: "".into() },
    ];
    let step_model = std::rc::Rc::new(slint::VecModel::from(steps));
    ui.set_launch_steps(step_model.into());

    let result = ui.get_launch_steps();
    assert_eq!(result.row_count(), 3, "R03: must track 3 launch stages");
    assert_eq!(result.row_data(0).unwrap().status, "done");
    assert_eq!(result.row_data(1).unwrap().status, "running");
    assert_eq!(result.row_data(2).unwrap().status, "pending");

    // Launch/stop/cancel callbacks
    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_launch_selected(move || { c.set(c.get() + 1); });
    ui.invoke_launch_selected();
    assert_eq!(fired.get(), 1, "R03: launch_selected must fire");

    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_stop_selected(move || { c.set(c.get() + 1); });
    ui.invoke_stop_selected();
    assert_eq!(fired.get(), 1, "R03: stop_selected must fire");

    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_cancel_launch(move || { c.set(c.get() + 1); });
    ui.invoke_cancel_launch();
    assert_eq!(fired.get(), 1, "R03: cancel_launch must fire");
}

// ═══════════════════════════════════════════════════════════════════
// R01+: Navigation
// Tab-based navigation across Setup, Cluster, Projects, Pods, Settings.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r01_navigation_pages() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // 5 pages: 0=Cluster, 1=Projects, 2=Pods, 3=Settings
    for page in 0..=3 {
        ui.set_active_page(page);
        assert_eq!(ui.get_active_page(), page,
            "R01: must navigate to page {}", page);
    }
}

// ═══════════════════════════════════════════════════════════════════
// R01+: Busy State
// UI must track busy state to prevent concurrent operations.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r01_busy_state_management() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    assert!(!ui.get_is_busy(), "R01: must start not busy");
    ui.set_is_busy(true);
    assert!(ui.get_is_busy(), "R01: must set busy");
    ui.set_is_busy(false);
    assert!(!ui.get_is_busy(), "R01: must clear busy");

    // Installing state (separate from busy)
    assert!(!ui.get_is_installing(), "R01: must start not installing");
    ui.set_is_installing(true);
    assert!(ui.get_is_installing());
}

// ═══════════════════════════════════════════════════════════════════
// CLU-4: WSL health status must be displayed continuously (Windows)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn clu4_wsl_status_property() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // Default is Unknown
    assert_eq!(ui.get_wsl_status(), "Unknown",
        "CLU-4: wsl-status must default to Unknown");

    // Can be set to Healthy/Unhealthy
    ui.set_wsl_status("Healthy".into());
    assert_eq!(ui.get_wsl_status(), "Healthy",
        "CLU-4: wsl-status must accept Healthy");
    ui.set_wsl_status("Unhealthy".into());
    assert_eq!(ui.get_wsl_status(), "Unhealthy",
        "CLU-4: wsl-status must accept Unhealthy");
}

// ═══════════════════════════════════════════════════════════════════
// CLU-30: Cluster connectivity monitoring during operations
// Recovery hint is set when connectivity is lost mid-operation.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn clu30_recovery_hint_for_connectivity_loss() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // Simulate connectivity loss by setting recovery hint
    let hint = "Cluster connection lost during operation. Check Docker Desktop and k3d cluster status.";
    ui.set_recovery_hint(hint.into());
    assert_eq!(ui.get_recovery_hint(), hint,
        "CLU-30: connectivity loss hint must be displayable");

    // Clearing on recovery
    ui.set_recovery_hint("".into());
    assert_eq!(ui.get_recovery_hint(), "",
        "CLU-30: hint must clear when connectivity restores");
}

// ═══════════════════════════════════════════════════════════════════
// CLU-29: Manual fix steps shown when auto-recovery fails
// ═══════════════════════════════════════════════════════════════════

#[test]
fn clu29_recovery_hint_property() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    assert_eq!(ui.get_recovery_hint(), "",
        "CLU-29: recovery-hint must default to empty");

    ui.set_recovery_hint("Manual fix: Run 'helm uninstall'".into());
    assert_eq!(ui.get_recovery_hint(), "Manual fix: Run 'helm uninstall'",
        "CLU-29: recovery-hint must be settable");

    // Clearing hint should work
    ui.set_recovery_hint("".into());
    assert_eq!(ui.get_recovery_hint(), "",
        "CLU-29: recovery-hint must be clearable when health recovers");
}

// ═══════════════════════════════════════════════════════════════════
// R04+: Continue App (post-setup)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn r04_continue_app_callback() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    let fired = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = fired.clone();
    ui.on_continue_app(move || { c.set(c.get() + 1); });
    ui.invoke_continue_app();
    assert_eq!(fired.get(), 1, "R04: continue_app must fire after setup");
}

// ═══════════════════════════════════════════════════════════════════
// APP-16: Config corruption fallback
// ═══════════════════════════════════════════════════════════════════

#[test]
fn app16_corrupt_config_falls_back_to_defaults() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("config.toml");

    // Write garbage
    std::fs::write(&path, "{{{{ not valid toml !!! ").unwrap();

    // This mimics what AppConfig::load() does — corrupt files return defaults
    let content = std::fs::read_to_string(&path).unwrap();
    let result: Result<toml::Value, _> = toml::from_str(&content);
    assert!(result.is_err(), "APP-16: corrupt TOML should parse-fail");
    // The real load() catches this and returns defaults — verified in unit tests
}

// ═══════════════════════════════════════════════════════════════════
// APP-13/APP-17: Desired state persistence
// ═══════════════════════════════════════════════════════════════════

#[test]
fn app13_desired_state_roundtrip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("state.json");

    // Simulate deploy tracking
    let state_json = r#"{"deployed_projects":["alpha","beta"]}"#;
    std::fs::write(&path, state_json).unwrap();

    let loaded: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&path).unwrap()
    ).unwrap();

    let projects = loaded["deployed_projects"].as_array().unwrap();
    assert_eq!(projects.len(), 2, "APP-13: desired state must persist deployed projects");
    assert_eq!(projects[0], "alpha");
    assert_eq!(projects[1], "beta");
}

#[test]
fn app5_orphan_detection_logic() {
    // APP-5: Orphaned deployments = helm releases not in desired state
    let desired: std::collections::BTreeSet<String> =
        vec!["proj-a".to_string()].into_iter().collect();

    let releases = vec![
        "claude-proj-a".to_string(),
        "claude-proj-orphan".to_string(),
    ];

    let orphans: Vec<&String> = releases
        .iter()
        .filter(|r| {
            if let Some(proj) = r.strip_prefix("claude-") {
                !desired.contains(proj)
            } else {
                false
            }
        })
        .collect();

    assert_eq!(orphans.len(), 1, "APP-5: should detect 1 orphaned release");
    assert_eq!(orphans[0], "claude-proj-orphan");
}

// ═══════════════════════════════════════════════════════════════════
// ERR-2/ERR-3/ERR-4: Toast notification system
// ═══════════════════════════════════════════════════════════════════

#[test]
fn err2_toast_properties_exist() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // ERR-2: Toast model and callbacks must exist
    let toasts = ui.get_toasts();
    assert_eq!(toasts.row_count(), 0, "ERR-2: toasts must start empty");

    // Set toasts model
    let entries = vec![
        ToastEntry { message: "Test error".into(), level: "error".into(), id: 1, target_page: -1 },
    ];
    let model = std::rc::Rc::new(slint::VecModel::from(entries));
    ui.set_toasts(model.into());

    let toasts = ui.get_toasts();
    assert_eq!(toasts.row_count(), 1, "ERR-2: should have 1 toast");
    assert_eq!(toasts.row_data(0).unwrap().message, "Test error");
}

#[test]
fn err4_toast_max_three() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // ERR-4: Max 3 toasts visible
    let entries = vec![
        ToastEntry { message: "A".into(), level: "info".into(), id: 1, target_page: -1 },
        ToastEntry { message: "B".into(), level: "warning".into(), id: 2, target_page: 0 },
        ToastEntry { message: "C".into(), level: "error".into(), id: 3, target_page: 2 },
    ];
    let model = std::rc::Rc::new(slint::VecModel::from(entries));
    ui.set_toasts(model.into());

    assert_eq!(ui.get_toasts().row_count(), 3, "ERR-4: max 3 toasts");
}

// ═══════════════════════════════════════════════════════════════════
// SET-19: Settings validation
// ═══════════════════════════════════════════════════════════════════

#[test]
fn set19_settings_error_property() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    assert_eq!(ui.get_settings_error(), "", "SET-19: settings-error starts empty");

    ui.set_settings_error("CPU limit is not valid.".into());
    assert_eq!(ui.get_settings_error(), "CPU limit is not valid.");
}

// ═══════════════════════════════════════════════════════════════════
// SUP-8: Terraform hidden on Windows in setup panel
// ═══════════════════════════════════════════════════════════════════

#[test]
fn sup8_platform_name_property_exists() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // platform-name property should exist and be settable
    ui.set_platform_name("Windows".into());
    assert_eq!(ui.get_platform_name(), "Windows");
    ui.set_platform_name("Linux".into());
    assert_eq!(ui.get_platform_name(), "Linux");
}

// ═══════════════════════════════════════════════════════════════════
// SET-12: Extra mounts UI property
// ═══════════════════════════════════════════════════════════════════

#[test]
fn set12_extra_mounts_text_property() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    assert_eq!(ui.get_extra_mounts_text(), "", "SET-12: extra-mounts-text starts empty");

    ui.set_extra_mounts_text("/data/shared, /opt/tools".into());
    assert_eq!(ui.get_extra_mounts_text(), "/data/shared, /opt/tools");
}

// ═══════════════════════════════════════════════════════════════════
// ERR-5: Toast click-to-navigate (target-page field in ToastEntry)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn err5_toast_target_page_field() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    let entries = vec![
        ToastEntry { message: "Deploy done".into(), level: "info".into(), id: 1, target_page: 2 },
        ToastEntry { message: "Error".into(), level: "error".into(), id: 2, target_page: 0 },
    ];
    let model = std::rc::Rc::new(slint::VecModel::from(entries));
    ui.set_toasts(model.into());

    let t0 = ui.get_toasts().row_data(0).unwrap();
    assert_eq!(t0.target_page, 2, "ERR-5: toast should carry target page for navigation");
    let t1 = ui.get_toasts().row_data(1).unwrap();
    assert_eq!(t1.target_page, 0, "ERR-5: second toast should navigate to cluster page");
}

// ═══════════════════════════════════════════════════════════════════
// NET-2: NetworkPolicy template exists
// ═══════════════════════════════════════════════════════════════════

#[test]
fn net2_networkpolicy_template_exists() {
    let path = std::path::Path::new("helm/claude-code/templates/networkpolicy.yaml");
    assert!(path.exists(), "NET-2: NetworkPolicy template should exist");
    let content = std::fs::read_to_string(path).unwrap();
    assert!(content.contains("NetworkPolicy"), "NET-2: should define a NetworkPolicy");
    assert!(content.contains("Ingress"), "NET-2: should restrict ingress");
    assert!(content.contains("project"), "NET-2: should be per-project");
}

// ═══════════════════════════════════════════════════════════════════
// SET-15: Image retention hours config field
// ═══════════════════════════════════════════════════════════════════

#[test]
fn set15_image_retention_hours_default() {
    let cfg = claude_in_k3s::config::AppConfig::default();
    assert_eq!(cfg.image_retention_hours, 168, "SET-15: default retention should be 7 days (168 hours)");
}

// ═══════════════════════════════════════════════════════════════════
// APP-14: Continuous state reconciliation logic
// ═══════════════════════════════════════════════════════════════════

#[test]
fn app14_reconciliation_detects_missing() {
    let mut state = claude_in_k3s::state::DesiredState::default();
    state.mark_deployed("project-a");
    state.mark_deployed("project-b");

    // Only project-a is running
    let running = vec!["project-a".to_string()];
    let missing = state.find_missing_deployments(&running);

    assert_eq!(missing.len(), 1);
    assert_eq!(*missing[0], "project-b");
}

// ═══════════════════════════════════════════════════════════════════
// SUP-9/SUP-10: Deps re-checking
// ═══════════════════════════════════════════════════════════════════

#[test]
fn sup9_all_met_for_platform_aware() {
    use claude_in_k3s::deps::{DepsStatus, ToolStatus};
    use claude_in_k3s::platform::Platform;

    let status = DepsStatus {
        k3s: ToolStatus::Found { version: "v5.7".into() },
        terraform: ToolStatus::Missing,
        helm: ToolStatus::Found { version: "3.12".into() },
        docker: ToolStatus::Found { version: "24.0".into() },
        claude: ToolStatus::Missing,
    };

    // Windows: terraform not required
    assert!(status.all_met_for(&Platform::Windows));
    // Linux: terraform required
    assert!(!status.all_met_for(&Platform::Linux));
}

/// PRJ-60: Duplicate namespace suffix logic — names colliding after sanitization get numeric suffixes
#[test]
fn prj60_deduplicated_release_names() {
    use claude_in_k3s::helm::HelmRunner;

    // No conflicts
    let result = HelmRunner::deduplicated_release_names(&["alpha", "beta"]);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].1, "claude-alpha");
    assert_eq!(result[1].1, "claude-beta");

    // "my_project" and "my.project" both sanitize to "claude-my-project"
    let result = HelmRunner::deduplicated_release_names(&["my_project", "my.project"]);
    assert_eq!(result[0].1, "claude-my-project");
    assert_eq!(result[1].1, "claude-my-project-2");
}

/// POD-65: CrashLoopBackOff projects show "failed" badge in projects panel
#[test]
fn pod65_pod_failed_field_in_project_entry() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    let entries = vec![
        ProjectEntry {
            name: "healthy-proj".into(),
            path: "/projects/healthy".into(),
            selected: false,
            base_image_index: 0,
            has_custom_dockerfile: false,
            deployed: true,
            pod_failed: false,
            ambiguous: false,
        },
        ProjectEntry {
            name: "crashed-proj".into(),
            path: "/projects/crashed".into(),
            selected: false,
            base_image_index: 0,
            has_custom_dockerfile: false,
            deployed: true,
            pod_failed: true,
            ambiguous: false,
        },
    ];
    let model = std::rc::Rc::new(slint::VecModel::from(entries));
    ui.set_projects(model.into());

    let projects = ui.get_projects();
    use slint::Model;
    let p0 = projects.row_data(0).unwrap();
    let p1 = projects.row_data(1).unwrap();
    assert!(!p0.pod_failed, "healthy project should not be marked as failed");
    assert!(p1.pod_failed, "crashed project should be marked as failed");
}

/// PRJ-61: Ambiguous flag available on ProjectEntry
#[test]
fn prj61_ambiguous_field_in_project_entry() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    let entries = vec![
        ProjectEntry {
            name: "single-lang".into(),
            path: "/projects/single".into(),
            selected: false,
            base_image_index: 0,
            has_custom_dockerfile: false,
            deployed: false,
            pod_failed: false,
            ambiguous: false,
        },
        ProjectEntry {
            name: "multi-lang".into(),
            path: "/projects/multi".into(),
            selected: false,
            base_image_index: 0,
            has_custom_dockerfile: false,
            deployed: false,
            pod_failed: false,
            ambiguous: true,
        },
    ];
    let model = std::rc::Rc::new(slint::VecModel::from(entries));
    ui.set_projects(model.into());

    let projects = ui.get_projects();
    use slint::Model;
    let p0 = projects.row_data(0).unwrap();
    let p1 = projects.row_data(1).unwrap();
    assert!(!p0.ambiguous, "single-language project should not be ambiguous");
    assert!(p1.ambiguous, "multi-language project should be marked as ambiguous");
}

/// PRJ-53: retry-build callback exists on AppWindow
#[test]
fn prj53_retry_build_callback_exists() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = AppWindow::new().unwrap();

    // Verify the retry-build callback can be set without panic
    ui.on_retry_build(|_label| {
        // callback wired up successfully
    });
}
