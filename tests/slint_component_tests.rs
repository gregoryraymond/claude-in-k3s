//! Component-level Slint tests using inline slint! definitions.
//! Each module isolates a specific UI pattern and verifies its logic
//! without requiring the full AppWindow or a display server.

/// Tests for icon button computed properties and state logic.
mod icon_button_logic {
    slint::slint! {
        export component TestIconButton inherits Window {
            in property <string> label: "";
            in property <string> tooltip: "";
            in property <bool> enabled: true;
            in property <bool> compact: false;
            in property <bool> primary: false;
            in property <bool> destructive: false;

            // Expose computed properties for testing
            out property <string> computed-tip-text: tooltip != "" ? tooltip : label;
            out property <bool> computed-has-label: label != "";
            out property <float> computed-opacity: enabled ? 1.0 : 0.4;
            out property <length> computed-height: compact ? 28px : 34px;
            out property <length> computed-min-width: compact ? 28px : 72px;

            callback clicked();
        }
    }

    #[test]
    fn tooltip_falls_back_to_label() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestIconButton::new().unwrap();

        ui.set_label("Delete".into());
        ui.set_tooltip("".into());
        assert_eq!(ui.get_computed_tip_text(), "Delete");
    }

    #[test]
    fn tooltip_overrides_label() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestIconButton::new().unwrap();

        ui.set_label("Del".into());
        ui.set_tooltip("Delete this pod".into());
        assert_eq!(ui.get_computed_tip_text(), "Delete this pod");
    }

    #[test]
    fn disabled_reduces_opacity() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestIconButton::new().unwrap();

        assert_eq!(ui.get_computed_opacity(), 1.0);
        ui.set_enabled(false);
        assert!((ui.get_computed_opacity() - 0.4).abs() < 0.01);
    }

    #[test]
    fn compact_changes_dimensions() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestIconButton::new().unwrap();

        // Standard mode
        assert_eq!(ui.get_computed_height(), 34.0);
        assert_eq!(ui.get_computed_min_width(), 72.0);

        // Compact mode
        ui.set_compact(true);
        assert_eq!(ui.get_computed_height(), 28.0);
        assert_eq!(ui.get_computed_min_width(), 28.0);
    }

    #[test]
    fn callback_fires() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestIconButton::new().unwrap();

        let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let c = counter.clone();
        ui.on_clicked(move || { c.set(c.get() + 1); });
        ui.invoke_clicked();
        assert_eq!(counter.get(), 1);
    }
}

/// Tests for status badge color mapping logic.
mod status_badge_logic {
    slint::slint! {
        export global Theme {
            out property <color> success: #4ade80;
            out property <color> warning: #fbbf24;
            out property <color> error:   #f87171;
            out property <color> surface-alt: #16162a;
        }

        export component TestStatusBadge inherits Window {
            in property <string> status: "";

            out property <color> computed-bg: {
                if status == "Running" || status == "Healthy" || status == "Found" { return Theme.success; }
                if status == "Pending" || status == "Starting" || status == "Degraded" || status == "ContainerCreating" { return Theme.warning; }
                if status == "Failed" || status == "Error" || status == "Missing" || status == "CrashLoopBackOff" || status == "ImagePullBackOff" || status == "ErrImageNeverPull" { return Theme.error; }
                return Theme.surface-alt;
            };

            out property <bool> is-warning-text: status == "Pending" || status == "Starting" || status == "Degraded" || status == "ContainerCreating";
        }
    }

    #[test]
    fn success_statuses() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestStatusBadge::new().unwrap();

        let success = slint::Color::from_argb_u8(255, 0x4a, 0xde, 0x80);

        for status in &["Running", "Healthy", "Found"] {
            ui.set_status((*status).into());
            assert_eq!(ui.get_computed_bg(), success, "expected success for '{}'", status);
        }
    }

    #[test]
    fn warning_statuses() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestStatusBadge::new().unwrap();

        let warning = slint::Color::from_argb_u8(255, 0xfb, 0xbf, 0x24);

        for status in &["Pending", "Starting", "Degraded", "ContainerCreating"] {
            ui.set_status((*status).into());
            assert_eq!(ui.get_computed_bg(), warning, "expected warning for '{}'", status);
            assert!(ui.get_is_warning_text(), "expected dark text for '{}'", status);
        }
    }

    #[test]
    fn error_statuses() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestStatusBadge::new().unwrap();

        let error = slint::Color::from_argb_u8(255, 0xf8, 0x71, 0x71);

        for status in &["Failed", "Error", "Missing", "CrashLoopBackOff", "ImagePullBackOff", "ErrImageNeverPull"] {
            ui.set_status((*status).into());
            assert_eq!(ui.get_computed_bg(), error, "expected error for '{}'", status);
        }
    }

    #[test]
    fn unknown_status_uses_default() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestStatusBadge::new().unwrap();

        let default = slint::Color::from_argb_u8(255, 0x16, 0x16, 0x2a);

        ui.set_status("Unknown".into());
        assert_eq!(ui.get_computed_bg(), default);

        ui.set_status("".into());
        assert_eq!(ui.get_computed_bg(), default);

        ui.set_status("SomeRandomStatus".into());
        assert_eq!(ui.get_computed_bg(), default);
    }
}

/// Tests for cluster panel conditional logic (deploy/redeploy label, button states).
mod cluster_panel_logic {
    slint::slint! {
        export component TestClusterPanel inherits Window {
            in property <string> cluster-status: "Unknown";
            in property <bool> is-busy: false;
            in property <bool> tf-initialized: false;

            out property <string> deploy-label: cluster-status == "Healthy" ? "Redeploy" : "Deploy";
            out property <bool> deploy-enabled: !is-busy;
            out property <bool> destroy-enabled: !is-busy && tf-initialized;
        }
    }

    #[test]
    fn deploy_label_changes_with_status() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestClusterPanel::new().unwrap();

        assert_eq!(ui.get_deploy_label(), "Deploy");

        ui.set_cluster_status("Healthy".into());
        assert_eq!(ui.get_deploy_label(), "Redeploy");

        ui.set_cluster_status("Stopped".into());
        assert_eq!(ui.get_deploy_label(), "Deploy");
    }

    #[test]
    fn buttons_disabled_when_busy() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestClusterPanel::new().unwrap();

        assert!(ui.get_deploy_enabled());
        ui.set_is_busy(true);
        assert!(!ui.get_deploy_enabled());
    }

    #[test]
    fn destroy_requires_tf_initialized() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestClusterPanel::new().unwrap();

        assert!(!ui.get_destroy_enabled(), "destroy should be disabled without tf init");
        ui.set_tf_initialized(true);
        assert!(ui.get_destroy_enabled());
        ui.set_is_busy(true);
        assert!(!ui.get_destroy_enabled(), "destroy should be disabled when busy");
    }
}

/// Tests for WSL stack layer visibility and status (CLU-4).
mod wsl_stack_layer_logic {
    slint::slint! {
        export component TestWslStack inherits Window {
            in property <string> platform-name: "Windows";
            in property <string> wsl-status: "Unknown";
            in property <string> docker-status: "Unknown";

            out property <bool> wsl-ok: wsl-status == "Healthy" || wsl-status == "Unknown";
            out property <bool> docker-ok: docker-status == "Running" || docker-status == "Healthy";
            out property <bool> show-wsl: platform-name == "Windows";
            out property <bool> docker-dimmed: platform-name == "Windows" && !wsl-ok;
        }
    }

    #[test]
    fn wsl_visible_on_windows() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestWslStack::new().unwrap();
        assert!(ui.get_show_wsl(), "WSL layer should be visible on Windows");
    }

    #[test]
    fn wsl_hidden_on_linux() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestWslStack::new().unwrap();
        ui.set_platform_name("Linux".into());
        assert!(!ui.get_show_wsl(), "WSL layer should be hidden on Linux");
    }

    #[test]
    fn docker_dimmed_when_wsl_unhealthy() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestWslStack::new().unwrap();
        ui.set_wsl_status("Unhealthy".into());
        assert!(ui.get_docker_dimmed(), "Docker should be dimmed when WSL is unhealthy on Windows");
    }

    #[test]
    fn docker_not_dimmed_when_wsl_healthy() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestWslStack::new().unwrap();
        ui.set_wsl_status("Healthy".into());
        assert!(!ui.get_docker_dimmed());
    }

    #[test]
    fn docker_not_dimmed_on_linux_regardless_of_wsl() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestWslStack::new().unwrap();
        ui.set_platform_name("Linux".into());
        ui.set_wsl_status("Unhealthy".into());
        assert!(!ui.get_docker_dimmed(), "Docker should never be dimmed on Linux");
    }
}

/// Tests for recovery hint visibility (CLU-29).
mod recovery_hint_logic {
    slint::slint! {
        export component TestRecoveryHint inherits Window {
            in property <string> recovery-hint: "";

            out property <bool> show-hint: recovery-hint != "";
        }
    }

    #[test]
    fn hint_hidden_when_empty() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestRecoveryHint::new().unwrap();
        assert!(!ui.get_show_hint());
    }

    #[test]
    fn hint_shown_when_set() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestRecoveryHint::new().unwrap();
        ui.set_recovery_hint("Manual fix: delete namespace".into());
        assert!(ui.get_show_hint());
    }

    #[test]
    fn hint_hidden_again_after_clear() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestRecoveryHint::new().unwrap();
        ui.set_recovery_hint("some hint".into());
        assert!(ui.get_show_hint());
        ui.set_recovery_hint("".into());
        assert!(!ui.get_show_hint());
    }
}

/// Tests for setup panel dependency-driven button states.
mod setup_panel_logic {
    slint::slint! {
        export component TestSetupPanel inherits Window {
            in property <bool> k3s-found: false;
            in property <bool> terraform-found: false;
            in property <bool> helm-found: false;
            in property <bool> docker-found: false;
            in property <bool> is-installing: false;

            out property <bool> install-enabled: !is-installing && !(k3s-found && terraform-found && helm-found && docker-found);
            out property <bool> continue-enabled: !is-installing && k3s-found && terraform-found && helm-found && docker-found;
            out property <string> install-label: is-installing ? "Installing..." : "Install Missing";
        }
    }

    #[test]
    fn install_enabled_when_deps_missing() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestSetupPanel::new().unwrap();

        // Nothing found -> install enabled
        assert!(ui.get_install_enabled());
        assert!(!ui.get_continue_enabled());
    }

    #[test]
    fn install_disabled_when_all_found() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestSetupPanel::new().unwrap();

        ui.set_k3s_found(true);
        ui.set_terraform_found(true);
        ui.set_helm_found(true);
        ui.set_docker_found(true);

        assert!(!ui.get_install_enabled(), "install should be disabled when all deps found");
        assert!(ui.get_continue_enabled(), "continue should be enabled when all deps found");
    }

    #[test]
    fn install_disabled_during_installation() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestSetupPanel::new().unwrap();

        ui.set_is_installing(true);
        assert!(!ui.get_install_enabled());
        assert!(!ui.get_continue_enabled());
        assert_eq!(ui.get_install_label(), "Installing...");
    }

    #[test]
    fn partial_deps_still_needs_install() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestSetupPanel::new().unwrap();

        ui.set_k3s_found(true);
        ui.set_docker_found(true);
        // terraform and helm still missing
        assert!(ui.get_install_enabled());
        assert!(!ui.get_continue_enabled());
    }
}

/// Tests for log viewer auto-scroll toggle logic.
mod log_viewer_logic {
    slint::slint! {
        export component TestLogViewer inherits Window {
            in property <string> text: "";
            in-out property <bool> auto-scroll: true;

            out property <string> toggle-label: auto-scroll ? "Auto-scroll: ON" : "Auto-scroll: OFF";
        }
    }

    #[test]
    fn auto_scroll_defaults_on() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestLogViewer::new().unwrap();

        assert!(ui.get_auto_scroll());
        assert_eq!(ui.get_toggle_label(), "Auto-scroll: ON");
    }

    #[test]
    fn toggle_auto_scroll() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestLogViewer::new().unwrap();

        ui.set_auto_scroll(false);
        assert_eq!(ui.get_toggle_label(), "Auto-scroll: OFF");

        ui.set_auto_scroll(true);
        assert_eq!(ui.get_toggle_label(), "Auto-scroll: ON");
    }
}

/// Tests for pod selection conditional display logic.
mod pods_panel_logic {
    slint::slint! {
        export component TestPodsPanel inherits Window {
            in property <int> selected-pod-count: 0;
            in property <bool> is-busy: false;
            in-out property <string> claude-prompt: "";
            in property <string> claude-target-pod: "";
            in property <string> pod-log: "";

            out property <bool> show-action-bar: selected-pod-count > 0;
            out property <bool> show-prompt: claude-target-pod != "";
            out property <bool> show-log: pod-log != "";
            out property <bool> send-enabled: claude-prompt != "";
            out property <string> count-text: selected-pod-count + " selected";
        }
    }

    #[test]
    fn action_bar_hidden_when_none_selected() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestPodsPanel::new().unwrap();

        assert!(!ui.get_show_action_bar());
        ui.set_selected_pod_count(2);
        assert!(ui.get_show_action_bar());
        assert_eq!(ui.get_count_text(), "2 selected");
    }

    #[test]
    fn prompt_shown_only_with_target_pod() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestPodsPanel::new().unwrap();

        assert!(!ui.get_show_prompt());
        ui.set_claude_target_pod("my-pod".into());
        assert!(ui.get_show_prompt());
    }

    #[test]
    fn log_viewer_shown_only_with_content() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestPodsPanel::new().unwrap();

        assert!(!ui.get_show_log());
        ui.set_pod_log("some logs here".into());
        assert!(ui.get_show_log());
    }

    #[test]
    fn send_disabled_with_empty_prompt() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestPodsPanel::new().unwrap();

        assert!(!ui.get_send_enabled());
        ui.set_claude_prompt("hello".into());
        assert!(ui.get_send_enabled());
    }
}

/// Tests for projects panel conditional display logic.
mod projects_panel_logic {
    slint::slint! {
        export component TestProjectsPanel inherits Window {
            in property <string> projects-dir: "";
            in property <bool> all-selected: false;
            in property <bool> is-busy: false;

            out property <bool> refresh-enabled: projects-dir != "";
            out property <string> select-label: all-selected ? "Deselect All" : "Select All";
            out property <string> dir-display: projects-dir == "" ? "No directory selected" : projects-dir;
        }
    }

    #[test]
    fn refresh_disabled_without_dir() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestProjectsPanel::new().unwrap();

        assert!(!ui.get_refresh_enabled());
        assert_eq!(ui.get_dir_display(), "No directory selected");
    }

    #[test]
    fn refresh_enabled_with_dir() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestProjectsPanel::new().unwrap();

        ui.set_projects_dir("/home/user/projects".into());
        assert!(ui.get_refresh_enabled());
        assert_eq!(ui.get_dir_display(), "/home/user/projects");
    }

    #[test]
    fn select_all_label_toggles() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestProjectsPanel::new().unwrap();

        assert_eq!(ui.get_select_label(), "Select All");
        ui.set_all_selected(true);
        assert_eq!(ui.get_select_label(), "Deselect All");
    }
}

/// Tests for pod row display logic (ready text, restart color threshold).
mod pod_row_logic {
    slint::slint! {
        export component TestPodRow inherits Window {
            in property <bool> ready: false;
            in property <int> restart-count: 0;
            in property <bool> exposed: false;

            out property <string> ready-text: ready ? "Yes" : "No";
            out property <bool> restarts-critical: restart-count > 3;
            out property <string> network-tooltip: exposed ? "Unexpose" : "Expose";
        }
    }

    #[test]
    fn ready_text() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestPodRow::new().unwrap();

        assert_eq!(ui.get_ready_text(), "No");
        ui.set_ready(true);
        assert_eq!(ui.get_ready_text(), "Yes");
    }

    #[test]
    fn restart_count_threshold() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestPodRow::new().unwrap();

        ui.set_restart_count(0);
        assert!(!ui.get_restarts_critical());
        ui.set_restart_count(3);
        assert!(!ui.get_restarts_critical());
        ui.set_restart_count(4);
        assert!(ui.get_restarts_critical());
    }

    #[test]
    fn network_tooltip_reflects_exposure() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestPodRow::new().unwrap();

        assert_eq!(ui.get_network_tooltip(), "Expose");
        ui.set_exposed(true);
        assert_eq!(ui.get_network_tooltip(), "Unexpose");
    }
}

/// Tests for memory bar color thresholds (CLU-7).
/// Replicates the MemoryBar color logic from cluster-panel.slint:
///   percent > 90 → error (red)
///   percent > 70 → warning (yellow)
///   else         → accent (green/copper)
mod memory_bar_color_thresholds {
    slint::slint! {
        export global Theme {
            out property <color> accent:  #d97706;
            out property <color> warning: #fbbf24;
            out property <color> error:   #ef4444;
        }

        export component TestMemoryBar inherits Window {
            in property <int> percent: 0;

            out property <color> bar-color: {
                if percent > 90 { return Theme.error; }
                if percent > 70 { return Theme.warning; }
                return Theme.accent;
            };
        }
    }

    #[test]
    fn percent_50_is_accent() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestMemoryBar::new().unwrap();
        let accent = slint::Color::from_argb_u8(255, 0xd9, 0x77, 0x06);

        ui.set_percent(50);
        assert_eq!(ui.get_bar_color(), accent, "50% should be accent (green/copper)");
    }

    #[test]
    fn percent_70_is_accent_boundary() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestMemoryBar::new().unwrap();
        let accent = slint::Color::from_argb_u8(255, 0xd9, 0x77, 0x06);

        ui.set_percent(70);
        assert_eq!(ui.get_bar_color(), accent, "70% should still be accent (boundary)");
    }

    #[test]
    fn percent_71_is_warning() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestMemoryBar::new().unwrap();
        let warning = slint::Color::from_argb_u8(255, 0xfb, 0xbf, 0x24);

        ui.set_percent(71);
        assert_eq!(ui.get_bar_color(), warning, "71% should be warning (yellow)");
    }

    #[test]
    fn percent_90_is_warning_boundary() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestMemoryBar::new().unwrap();
        let warning = slint::Color::from_argb_u8(255, 0xfb, 0xbf, 0x24);

        ui.set_percent(90);
        assert_eq!(ui.get_bar_color(), warning, "90% should still be warning (boundary)");
    }

    #[test]
    fn percent_91_is_error() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestMemoryBar::new().unwrap();
        let error = slint::Color::from_argb_u8(255, 0xef, 0x44, 0x44);

        ui.set_percent(91);
        assert_eq!(ui.get_bar_color(), error, "91% should be error (red)");
    }

    #[test]
    fn percent_0_is_accent() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestMemoryBar::new().unwrap();
        let accent = slint::Color::from_argb_u8(255, 0xd9, 0x77, 0x06);

        ui.set_percent(0);
        assert_eq!(ui.get_bar_color(), accent, "0% should be accent");
    }

    #[test]
    fn percent_100_is_error() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestMemoryBar::new().unwrap();
        let error = slint::Color::from_argb_u8(255, 0xef, 0x44, 0x44);

        ui.set_percent(100);
        assert_eq!(ui.get_bar_color(), error, "100% should be error");
    }
}

/// Tests for pod total count display (POD-9).
mod pod_total_count {
    slint::slint! {
        export struct PodEntry {
            name: string,
            phase: string,
        }

        export component TestPodCount inherits Window {
            in property <[PodEntry]> pods: [];

            out property <int> pod-count: pods.length;
            out property <string> total-text: "Total: " + pods.length;
        }
    }

    #[test]
    fn empty_pods_count_is_zero() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestPodCount::new().unwrap();

        assert_eq!(ui.get_pod_count(), 0);
        assert_eq!(ui.get_total_text(), "Total: 0");
    }

    #[test]
    fn pods_length_matches_model_size() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestPodCount::new().unwrap();

        let pods = std::rc::Rc::new(slint::VecModel::from(vec![
            PodEntry { name: "pod-a".into(), phase: "Running".into() },
            PodEntry { name: "pod-b".into(), phase: "Pending".into() },
            PodEntry { name: "pod-c".into(), phase: "Running".into() },
        ]));
        ui.set_pods(slint::ModelRc::from(pods));

        assert_eq!(ui.get_pod_count(), 3);
        assert_eq!(ui.get_total_text(), "Total: 3");
    }
}

/// Tests for auto-scroll default and toggle (POD-17/18).
mod auto_scroll_default {
    slint::slint! {
        export component TestAutoScroll inherits Window {
            in-out property <bool> auto-scroll: true;

            out property <string> toggle-label: auto-scroll ? "Auto-scroll: ON" : "Auto-scroll: OFF";
        }
    }

    #[test]
    fn auto_scroll_defaults_to_true() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestAutoScroll::new().unwrap();

        assert!(ui.get_auto_scroll(), "auto-scroll should default to true");
        assert_eq!(ui.get_toggle_label(), "Auto-scroll: ON");
    }

    #[test]
    fn auto_scroll_can_be_toggled_off() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestAutoScroll::new().unwrap();

        ui.set_auto_scroll(false);
        assert!(!ui.get_auto_scroll());
        assert_eq!(ui.get_toggle_label(), "Auto-scroll: OFF");
    }

    #[test]
    fn auto_scroll_can_be_toggled_back_on() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestAutoScroll::new().unwrap();

        ui.set_auto_scroll(false);
        ui.set_auto_scroll(true);
        assert!(ui.get_auto_scroll());
        assert_eq!(ui.get_toggle_label(), "Auto-scroll: ON");
    }
}

/// Tests for restart count red threshold (POD-4).
/// The pods-panel.slint uses: color: pod.restart-count > 3 ? Theme.error : Theme.text
mod restart_count_threshold {
    slint::slint! {
        export global Theme {
            out property <color> text:  #d4d0cc;
            out property <color> error: #ef4444;
        }

        export component TestRestartCount inherits Window {
            in property <int> restart-count: 0;

            out property <color> restart-color: restart-count > 3 ? Theme.error : Theme.text;
            out property <bool> restarts-critical: restart-count > 3;
        }
    }

    #[test]
    fn zero_restarts_normal_color() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestRestartCount::new().unwrap();
        let text = slint::Color::from_argb_u8(255, 0xd4, 0xd0, 0xcc);

        ui.set_restart_count(0);
        assert_eq!(ui.get_restart_color(), text);
        assert!(!ui.get_restarts_critical());
    }

    #[test]
    fn three_restarts_still_normal() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestRestartCount::new().unwrap();
        let text = slint::Color::from_argb_u8(255, 0xd4, 0xd0, 0xcc);

        ui.set_restart_count(3);
        assert_eq!(ui.get_restart_color(), text, "3 restarts should be normal (boundary)");
        assert!(!ui.get_restarts_critical());
    }

    #[test]
    fn four_restarts_is_error() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestRestartCount::new().unwrap();
        let error = slint::Color::from_argb_u8(255, 0xef, 0x44, 0x44);

        ui.set_restart_count(4);
        assert_eq!(ui.get_restart_color(), error, "4 restarts should be error (red)");
        assert!(ui.get_restarts_critical());
    }

    #[test]
    fn high_restarts_is_error() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestRestartCount::new().unwrap();
        let error = slint::Color::from_argb_u8(255, 0xef, 0x44, 0x44);

        ui.set_restart_count(100);
        assert_eq!(ui.get_restart_color(), error);
        assert!(ui.get_restarts_critical());
    }
}

/// Tests for pod row port display (POD-7).
/// Logic: pod.exposed && pod.container-port > 0 ? ":" + port : pod.exposed ? "exposed" : "-"
mod pod_port_display {
    slint::slint! {
        export component TestPodPort inherits Window {
            in property <bool> exposed: false;
            in property <int> container-port: 0;

            out property <string> port-text: exposed && container-port > 0 ? ":" + container-port : exposed ? "exposed" : "-";
        }
    }

    #[test]
    fn not_exposed_shows_dash() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestPodPort::new().unwrap();

        assert_eq!(ui.get_port_text(), "-");
    }

    #[test]
    fn exposed_without_port_shows_exposed() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestPodPort::new().unwrap();

        ui.set_exposed(true);
        ui.set_container_port(0);
        assert_eq!(ui.get_port_text(), "exposed");
    }

    #[test]
    fn exposed_with_port_shows_colon_port() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestPodPort::new().unwrap();

        ui.set_exposed(true);
        ui.set_container_port(8080);
        assert_eq!(ui.get_port_text(), ":8080");
    }

    #[test]
    fn not_exposed_with_port_shows_dash() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestPodPort::new().unwrap();

        ui.set_exposed(false);
        ui.set_container_port(3000);
        assert_eq!(ui.get_port_text(), "-");
    }
}

/// Tests for action bar visibility (POD-12).
/// The selection action bar shows when selected-pod-count > 0.
mod action_bar_visibility {
    slint::slint! {
        export component TestActionBar inherits Window {
            in property <int> selected-pod-count: 0;

            out property <bool> show-action-bar: selected-pod-count > 0;
            out property <string> count-text: selected-pod-count + " selected";
        }
    }

    #[test]
    fn hidden_when_zero_selected() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestActionBar::new().unwrap();

        assert!(!ui.get_show_action_bar(), "action bar should be hidden when no pods selected");
    }

    #[test]
    fn visible_when_one_selected() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestActionBar::new().unwrap();

        ui.set_selected_pod_count(1);
        assert!(ui.get_show_action_bar());
        assert_eq!(ui.get_count_text(), "1 selected");
    }

    #[test]
    fn visible_when_many_selected() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestActionBar::new().unwrap();

        ui.set_selected_pod_count(5);
        assert!(ui.get_show_action_bar());
        assert_eq!(ui.get_count_text(), "5 selected");
    }

    #[test]
    fn hidden_again_when_deselected() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestActionBar::new().unwrap();

        ui.set_selected_pod_count(3);
        assert!(ui.get_show_action_bar());

        ui.set_selected_pod_count(0);
        assert!(!ui.get_show_action_bar());
    }
}

/// Tests for remote control active indicator (POD-28).
/// When active is true, the icon button uses active-bg background
/// and success color for icon colorize.
mod remote_control_indicator {
    slint::slint! {
        export global Theme {
            out property <color> text:         #d4d0cc;
            out property <color> text-muted:   #8a8480;
            out property <color> success:      #4ade80;
            out property <color> surface-alt:  #181614;
            out property <color> active-bg:    #1a2b1e;
        }

        export component TestRemoteControl inherits Window {
            in property <bool> remote-control: false;
            in property <bool> enabled: true;

            // Replicates icon-button.slint colorize logic for the active case
            out property <color> icon-color: {
                if !enabled { return Theme.text-muted; }
                if remote-control { return Theme.success; }
                return Theme.text;
            };

            // Replicates icon-button.slint background logic for active
            out property <color> bg-color: {
                if !enabled { return Theme.surface-alt; }
                if remote-control { return Theme.active-bg; }
                return Theme.surface-alt;
            };

            out property <string> tooltip-text: remote-control ? "Stop Remote Control" : "Start Remote Control";
        }
    }

    #[test]
    fn inactive_uses_normal_styling() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestRemoteControl::new().unwrap();
        let text = slint::Color::from_argb_u8(255, 0xd4, 0xd0, 0xcc);
        let surface_alt = slint::Color::from_argb_u8(255, 0x18, 0x16, 0x14);

        assert_eq!(ui.get_icon_color(), text);
        assert_eq!(ui.get_bg_color(), surface_alt);
        assert_eq!(ui.get_tooltip_text(), "Start Remote Control");
    }

    #[test]
    fn active_uses_success_color_and_active_bg() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestRemoteControl::new().unwrap();
        let success = slint::Color::from_argb_u8(255, 0x4a, 0xde, 0x80);
        let active_bg = slint::Color::from_argb_u8(255, 0x1a, 0x2b, 0x1e);

        ui.set_remote_control(true);
        assert_eq!(ui.get_icon_color(), success, "active should use success color");
        assert_eq!(ui.get_bg_color(), active_bg, "active should use active-bg");
        assert_eq!(ui.get_tooltip_text(), "Stop Remote Control");
    }

    #[test]
    fn disabled_overrides_active() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestRemoteControl::new().unwrap();
        let muted = slint::Color::from_argb_u8(255, 0x8a, 0x84, 0x80);
        let surface_alt = slint::Color::from_argb_u8(255, 0x18, 0x16, 0x14);

        ui.set_remote_control(true);
        ui.set_enabled(false);
        assert_eq!(ui.get_icon_color(), muted, "disabled should override active for icon color");
        assert_eq!(ui.get_bg_color(), surface_alt, "disabled should override active for background");
    }
}

/// Tests for launch tab status color mapping.
mod launch_tab_logic {
    slint::slint! {
        export global Theme {
            out property <color> success: #4ade80;
            out property <color> error:   #f87171;
            out property <color> warning: #fbbf24;
            out property <color> accent:  #6c63ff;
            out property <color> text-muted: #8888a0;
        }

        export component TestLaunchTab inherits Window {
            in property <string> status: "";

            out property <color> tab-color: {
                if status == "Done" { return Theme.success; }
                if status == "Failed" { return Theme.error; }
                if status == "Cancelled" { return Theme.warning; }
                if status == "Building" || status == "Importing" { return Theme.accent; }
                return Theme.text-muted;
            };
        }
    }

    #[test]
    fn tab_color_mapping() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestLaunchTab::new().unwrap();

        let success = slint::Color::from_argb_u8(255, 0x4a, 0xde, 0x80);
        let error = slint::Color::from_argb_u8(255, 0xf8, 0x71, 0x71);
        let warning = slint::Color::from_argb_u8(255, 0xfb, 0xbf, 0x24);
        let accent = slint::Color::from_argb_u8(255, 0x6c, 0x63, 0xff);
        let muted = slint::Color::from_argb_u8(255, 0x88, 0x88, 0xa0);

        ui.set_status("Done".into());
        assert_eq!(ui.get_tab_color(), success);

        ui.set_status("Failed".into());
        assert_eq!(ui.get_tab_color(), error);

        ui.set_status("Cancelled".into());
        assert_eq!(ui.get_tab_color(), warning);

        ui.set_status("Building".into());
        assert_eq!(ui.get_tab_color(), accent);

        ui.set_status("Importing".into());
        assert_eq!(ui.get_tab_color(), accent);

        ui.set_status("Pending".into());
        assert_eq!(ui.get_tab_color(), muted);
    }
}

/// ERR-2/ERR-3/ERR-4: Toast notification overlay logic.
mod toast_overlay_logic {
    slint::slint! {
        struct ToastEntry {
            message: string,
            level: string,
            id: int,
            target-page: int,
        }

        export component TestToastOverlay inherits Window {
            in property <[ToastEntry]> toasts: [];
            out property <int> toast-count: toasts.length;

            // Simulate level-based accent color logic
            out property <string> first-level: toasts.length > 0 ? toasts[0].level : "";
        }
    }

    #[test]
    fn toast_count_starts_at_zero() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestToastOverlay::new().unwrap();
        assert_eq!(ui.get_toast_count(), 0);
    }

    #[test]
    fn toast_count_reflects_model_size() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestToastOverlay::new().unwrap();

        let entries = vec![
            ToastEntry { message: "Error 1".into(), level: "error".into(), id: 1, target_page: -1 },
            ToastEntry { message: "Warning".into(), level: "warning".into(), id: 2, target_page: -1 },
        ];
        let model = std::rc::Rc::new(slint::VecModel::from(entries));
        ui.set_toasts(model.into());

        assert_eq!(ui.get_toast_count(), 2);
    }

    #[test]
    fn toast_max_three_enforced_by_caller() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestToastOverlay::new().unwrap();

        // ERR-4: Caller caps at 3
        let entries = vec![
            ToastEntry { message: "A".into(), level: "info".into(), id: 1, target_page: -1 },
            ToastEntry { message: "B".into(), level: "info".into(), id: 2, target_page: -1 },
            ToastEntry { message: "C".into(), level: "info".into(), id: 3, target_page: -1 },
        ];
        let model = std::rc::Rc::new(slint::VecModel::from(entries));
        ui.set_toasts(model.into());

        assert_eq!(ui.get_toast_count(), 3);
    }

    #[test]
    fn toast_level_accessible() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestToastOverlay::new().unwrap();

        let entries = vec![
            ToastEntry { message: "Fail".into(), level: "error".into(), id: 1, target_page: 2 },
        ];
        let model = std::rc::Rc::new(slint::VecModel::from(entries));
        ui.set_toasts(model.into());

        assert_eq!(ui.get_first_level(), "error");
    }

    // ERR-5: target-page accessible from model
    #[test]
    fn toast_target_page_accessible() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestToastOverlay::new().unwrap();

        let entries = vec![
            ToastEntry { message: "Navigate me".into(), level: "info".into(), id: 1, target_page: 2 },
        ];
        let model = std::rc::Rc::new(slint::VecModel::from(entries));
        ui.set_toasts(model.into());

        // Access via Model trait
        use slint::Model;
        let t = ui.get_toasts().row_data(0).unwrap();
        assert_eq!(t.target_page, 2);
    }
}

/// SUP-8: Setup panel hides terraform on Windows
mod setup_panel_platform {
    slint::slint! {
        component TestSetupPanel inherits Window {
            in property <string> platform-name: "Linux";
            in property <bool> k3s-found: true;
            in property <bool> terraform-found: false;
            in property <bool> helm-found: true;
            in property <bool> docker-found: true;

            // Mirror the button enable logic from setup-panel.slint
            out property <bool> install-enabled:
                !(k3s-found && (platform-name == "Windows" || terraform-found) && helm-found && docker-found);
            out property <bool> continue-enabled:
                k3s-found && (platform-name == "Windows" || terraform-found) && helm-found && docker-found;
            out property <bool> terraform-visible: platform-name != "Windows";
        }
    }

    #[test]
    fn terraform_hidden_on_windows() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestSetupPanel::new().unwrap();
        ui.set_platform_name("Windows".into());
        assert!(!ui.get_terraform_visible());
    }

    #[test]
    fn terraform_visible_on_linux() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestSetupPanel::new().unwrap();
        ui.set_platform_name("Linux".into());
        assert!(ui.get_terraform_visible());
    }

    #[test]
    fn continue_enabled_on_windows_without_terraform() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestSetupPanel::new().unwrap();
        ui.set_platform_name("Windows".into());
        ui.set_k3s_found(true);
        ui.set_terraform_found(false);
        ui.set_helm_found(true);
        ui.set_docker_found(true);
        assert!(ui.get_continue_enabled(), "SUP-8: Windows should not require terraform");
    }

    #[test]
    fn continue_disabled_on_linux_without_terraform() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestSetupPanel::new().unwrap();
        ui.set_platform_name("Linux".into());
        ui.set_k3s_found(true);
        ui.set_terraform_found(false);
        ui.set_helm_found(true);
        ui.set_docker_found(true);
        assert!(!ui.get_continue_enabled(), "SUP-8: Linux should require terraform");
    }
}
