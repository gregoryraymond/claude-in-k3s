use crate::config::AppConfig;
use crate::deps::DepsStatus;
use crate::docker::DockerBuilder;
use crate::error::AppResult;
use crate::helm::HelmRunner;
use crate::kubectl::{KubectlRunner, PodStatus};
use crate::platform::{self, Platform};
use crate::projects::{self, Project};
use crate::recovery;
use crate::state::DesiredState;
use crate::terraform::TerraformRunner;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use sysinfo::System;

/// Shared application state
pub struct AppState {
    pub config: AppConfig,
    pub platform: Platform,
    pub projects: Vec<Project>,
    pub pods: Vec<PodStatus>,
    pub cluster_healthy: bool,
    pub tf_initialized: bool,
    pub deps_status: DepsStatus,
    pub log_buffer: String,
    pub cancel_flag: Arc<AtomicBool>,
    pub syncing_ui: bool,
    /// Background `claude /remote-control` processes keyed by pod name.
    pub remote_control_procs: HashMap<String, u32>,
    /// POD-59: Pod names that should have remote-control sessions (persisted intent).
    pub remote_control_desired: std::collections::HashSet<String>,
    /// POD-67: Auto-restart attempt count per pod name.
    pub remote_control_restart_count: HashMap<String, u32>,
    pub recovery_tracker: recovery::RecoveryTracker,
    /// UIQ-6: Resource-level lock tracking (project/pod names currently being operated on).
    pub busy_resources: std::collections::HashSet<String>,
    /// POD-30: Currently active log-tailing process PID (killed when viewing different pod).
    pub log_tail_pid: Option<u32>,
    /// Projects whose deploy was interrupted (for CLU-32 resume).
    pub pending_deploy: Vec<String>,
    /// APP-13/APP-17: Persistent desired-state record.
    pub desired_state: DesiredState,
    /// PRJ-30: Cached cluster memory usage from latest health check.
    pub cluster_memory_used_mb: Option<u64>,
    pub cluster_memory_limit_mb: Option<u64>,
    project_root: PathBuf,
}

impl AppState {
    pub fn new() -> AppResult<Self> {
        let config = AppConfig::load()?;
        let plat = platform::detect_platform();
        // Start with all deps as Missing; checked async on a background thread
        let deps_status = DepsStatus::default();

        let project_root = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        // Try to find the project root by looking for Cargo.toml
        let project_root = find_project_root().unwrap_or(project_root);

        // APP-13: Load desired state from persistent file
        let desired_state = DesiredState::load();

        Ok(Self {
            config,
            platform: plat,
            projects: vec![],
            pods: vec![],
            cluster_healthy: false,
            tf_initialized: false,
            deps_status,
            log_buffer: String::new(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            syncing_ui: false,
            remote_control_procs: HashMap::new(),
            remote_control_desired: std::collections::HashSet::new(),
            remote_control_restart_count: HashMap::new(),
            recovery_tracker: recovery::RecoveryTracker::new(),
            busy_resources: std::collections::HashSet::new(),
            log_tail_pid: None,
            pending_deploy: Vec::new(),
            desired_state,
            cluster_memory_used_mb: None,
            cluster_memory_limit_mb: None,
            project_root,
        })
    }

    /// UIQ-6: Try to acquire a lock on a resource name. Returns false if already locked.
    pub fn try_lock_resource(&mut self, name: &str) -> bool {
        if self.busy_resources.contains(name) {
            false
        } else {
            self.busy_resources.insert(name.to_string());
            true
        }
    }

    /// UIQ-6: Release a resource lock.
    pub fn unlock_resource(&mut self, name: &str) {
        self.busy_resources.remove(name);
    }

    /// UIQ-6: Check if a resource is currently locked.
    pub fn is_resource_locked(&self, name: &str) -> bool {
        self.busy_resources.contains(name)
    }

    pub fn append_log(&mut self, msg: &str) {
        tracing::info!(target: "ui_log", "{}", msg);
        if !self.log_buffer.is_empty() {
            self.log_buffer.push('\n');
        }
        self.log_buffer.push_str(msg);
    }

    fn terraform_dir(&self) -> String {
        self.project_root
            .join(&self.config.terraform_dir)
            .to_string_lossy()
            .into()
    }

    fn helm_chart_dir(&self) -> String {
        self.project_root
            .join(&self.config.helm_chart_dir)
            .to_string_lossy()
            .into()
    }

    fn docker_dir(&self) -> String {
        self.project_root.join("docker").to_string_lossy().into()
    }

    /// Write `terraform.auto.tfvars` so Terraform knows the current platform.
    /// Called automatically before every terraform operation.
    pub fn write_terraform_vars(&self) -> AppResult<()> {
        let platform_str = match self.platform {
            Platform::Linux => "linux",
            Platform::MacOs => "macos",
            Platform::Wsl2 => "wsl2",
            Platform::Windows => "windows",
        };
        let vars_path = PathBuf::from(self.terraform_dir()).join("terraform.auto.tfvars");
        let memory_limit = self.compute_cluster_memory_limit();

        let mut content = format!(
            "platform = \"{}\"\ncluster_memory_limit = \"{}m\"\n",
            platform_str, memory_limit
        );

        // On Windows (k3d), generate volume mount flags so host paths
        // are accessible inside the k3d container.
        if self.platform == Platform::Windows {
            let mut volume_flags = Vec::new();

            // Mount projects directory if configured
            if let Some(ref projects_dir) = self.config.projects_dir {
                let container_path =
                    platform::to_k3d_container_path(projects_dir, &self.platform);
                volume_flags.push(platform::k3d_volume_flag(projects_dir, &container_path));
            }

            // Mount credentials directory (~/.claude)
            if let Some(home) = dirs::home_dir() {
                let claude_dir = home.join(".claude");
                let host_path = claude_dir.to_string_lossy().to_string();
                let container_path =
                    platform::to_k3d_container_path(&host_path, &self.platform);
                volume_flags.push(platform::k3d_volume_flag(&host_path, &container_path));
            }

            if !volume_flags.is_empty() {
                content.push_str(&format!(
                    "k3d_volume_mounts = {}\n",
                    serde_json::to_string(&volume_flags).unwrap_or_else(|_| "[]".into())
                ));
            }
        }

        std::fs::write(&vars_path, content)?;
        Ok(())
    }

    /// Compute the cluster memory limit in megabytes based on system RAM and config percentage.
    pub fn compute_cluster_memory_limit(&self) -> u64 {
        let mut sys = System::new();
        sys.refresh_memory();
        let total_mb = sys.total_memory() / 1024 / 1024;
        let percent = self.config.cluster_memory_percent.clamp(50, 95) as u64;
        total_mb * percent / 100
    }

    pub fn terraform_runner(&self) -> TerraformRunner {
        let _ = self.write_terraform_vars();
        TerraformRunner::new(
            platform::terraform_binary(&self.platform),
            &self.terraform_dir(),
        )
    }

    pub fn helm_runner(&self) -> HelmRunner {
        HelmRunner::new(
            platform::helm_binary(&self.platform),
            &self.helm_chart_dir(),
            "claude-code",
        )
    }

    pub fn kubectl_runner(&self) -> KubectlRunner {
        KubectlRunner::new(platform::kubectl_binary(&self.platform), "claude-code")
    }

    pub fn docker_builder(&self) -> DockerBuilder {
        DockerBuilder::new(
            platform::docker_binary(&self.platform),
            &self.docker_dir(),
            &self.platform,
        )
    }

    pub fn scan_projects(&mut self) -> AppResult<()> {
        if let Some(ref dir) = self.config.projects_dir {
            self.projects = projects::scan_projects(Path::new(dir))?;
        }
        Ok(())
    }

    pub fn selected_projects(&self) -> Vec<&Project> {
        self.projects.iter().filter(|p| p.selected).collect()
    }

}

fn find_project_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("Cargo.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::BaseImage;
    use tempfile::TempDir;

    fn make_state() -> AppState {
        AppState {
            config: AppConfig::default(),
            platform: Platform::Linux,
            projects: vec![],
            pods: vec![],
            cluster_healthy: false,
            tf_initialized: false,
            deps_status: crate::deps::DepsStatus {
                k3s: crate::deps::ToolStatus::Missing,
                terraform: crate::deps::ToolStatus::Missing,
                helm: crate::deps::ToolStatus::Missing,
                docker: crate::deps::ToolStatus::Missing,
                claude: crate::deps::ToolStatus::Missing,
            },
            log_buffer: String::new(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            syncing_ui: false,
            remote_control_procs: HashMap::new(),
            remote_control_desired: std::collections::HashSet::new(),
            remote_control_restart_count: HashMap::new(),
            recovery_tracker: crate::recovery::RecoveryTracker::new(),
            busy_resources: std::collections::HashSet::new(),
            log_tail_pid: None,
            pending_deploy: Vec::new(),
            desired_state: DesiredState::default(),
            cluster_memory_used_mb: None,
            cluster_memory_limit_mb: None,
            project_root: PathBuf::from("/tmp/test-root"),
        }
    }

    fn make_project(name: &str, selected: bool) -> Project {
        Project {
            name: name.to_string(),
            path: PathBuf::from(format!("/tmp/{}", name)),
            selected,
            base_image: BaseImage::Base,
            has_custom_dockerfile: false,
            ambiguous: false,
        }
    }

    #[test]
    fn append_log_empty_buffer() {
        let mut state = make_state();
        state.append_log("first message");
        assert_eq!(state.log_buffer, "first message");
    }

    #[test]
    fn append_log_adds_newline() {
        let mut state = make_state();
        state.append_log("line one");
        state.append_log("line two");
        assert_eq!(state.log_buffer, "line one\nline two");
    }

    #[test]
    fn append_log_multiple() {
        let mut state = make_state();
        state.append_log("alpha");
        state.append_log("beta");
        state.append_log("gamma");
        assert_eq!(state.log_buffer, "alpha\nbeta\ngamma");
    }

    #[test]
    fn selected_projects_none_selected() {
        let mut state = make_state();
        state.projects = vec![
            make_project("a", false),
            make_project("b", false),
        ];
        let selected = state.selected_projects();
        assert!(selected.is_empty());
    }

    #[test]
    fn selected_projects_some_selected() {
        let mut state = make_state();
        state.projects = vec![
            make_project("a", true),
            make_project("b", false),
            make_project("c", true),
        ];
        let selected = state.selected_projects();
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].name, "a");
        assert_eq!(selected[1].name, "c");
    }

    #[test]
    fn scan_projects_with_dir_set() {
        let tmp = TempDir::new().expect("create temp dir");
        std::fs::create_dir(tmp.path().join("proj_one")).unwrap();
        std::fs::create_dir(tmp.path().join("proj_two")).unwrap();
        std::fs::create_dir(tmp.path().join("proj_three")).unwrap();

        let mut state = make_state();
        state.config.projects_dir = Some(tmp.path().to_string_lossy().into_owned());
        state.scan_projects().expect("scan_projects");
        assert_eq!(state.projects.len(), 3);
    }

    #[test]
    fn scan_projects_with_no_dir() {
        let mut state = make_state();
        assert!(state.config.projects_dir.is_none());
        state.scan_projects().expect("scan_projects");
        assert!(state.projects.is_empty());
    }

    #[test]
    fn initial_state_values() {
        let state = make_state();
        assert!(!state.cluster_healthy);
        assert!(!state.tf_initialized);
        assert!(state.log_buffer.is_empty());
        assert!(state.projects.is_empty());
        assert!(state.pods.is_empty());
    }

    #[test]
    fn initial_state_has_deps_status() {
        let state = make_state();
        // Verify deps_status field exists and is accessible
        let _ = state.deps_status.all_met();
    }

    #[test]
    fn find_project_root_finds_cargo_toml() {
        // We are running from the project directory which contains Cargo.toml,
        // so find_project_root() should return Some.
        let root = find_project_root();
        assert!(root.is_some(), "find_project_root should find Cargo.toml");
        let root = root.unwrap();
        assert!(root.join("Cargo.toml").exists());
    }

    #[test]
    fn write_terraform_vars_creates_file() {
        let tmp = TempDir::new().expect("create temp dir");
        let tf_dir = tmp.path().join("terraform");
        std::fs::create_dir(&tf_dir).unwrap();

        let mut state = make_state();
        state.config.terraform_dir = "terraform".to_string();
        state.project_root = tmp.path().to_path_buf();
        state.platform = Platform::Windows;

        state.write_terraform_vars().expect("write vars");
        let content = std::fs::read_to_string(tf_dir.join("terraform.auto.tfvars")).unwrap();
        assert!(content.contains("platform = \"windows\""));
        assert!(content.contains("cluster_memory_limit = \""));
    }

    #[test]
    fn write_terraform_vars_linux() {
        let tmp = TempDir::new().expect("create temp dir");
        let tf_dir = tmp.path().join("terraform");
        std::fs::create_dir(&tf_dir).unwrap();

        let mut state = make_state();
        state.config.terraform_dir = "terraform".to_string();
        state.project_root = tmp.path().to_path_buf();
        state.platform = Platform::Linux;

        state.write_terraform_vars().expect("write vars");
        let content = std::fs::read_to_string(tf_dir.join("terraform.auto.tfvars")).unwrap();
        assert!(content.contains("platform = \"linux\""));
        assert!(content.contains("cluster_memory_limit = \""));
    }

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

    // =========================================================================
    // AppState::new() — real constructor
    // =========================================================================

    #[test]
    fn appstate_new_initializes_defaults() {
        let state = AppState::new().expect("AppState::new should succeed");
        // config has expected defaults when no config file exists
        assert_eq!(state.config.terraform_dir, "terraform");
        assert_eq!(state.config.helm_chart_dir, "helm/claude-code");
        assert_eq!(state.config.cluster_memory_percent, 80);
        // platform is detected (enum variant, not a panic)
        let _ = state.platform; // accessing doesn't panic
        // projects and pods start empty
        assert!(state.projects.is_empty());
        assert!(state.pods.is_empty());
        // log buffer starts empty
        assert!(state.log_buffer.is_empty());
        // cluster starts unhealthy
        assert!(!state.cluster_healthy);
        assert!(!state.tf_initialized);
    }

    // =========================================================================
    // append_log — empty message
    // =========================================================================

    #[test]
    fn append_log_empty_message() {
        let mut state = make_state();
        state.append_log("");
        assert_eq!(state.log_buffer, "");
    }

    #[test]
    fn append_log_empty_then_nonempty() {
        let mut state = make_state();
        state.append_log("");
        // append_log("") pushes empty string to empty buffer, buffer stays ""
        // Next append sees empty buffer so no newline prefix
        state.append_log("second");
        assert_eq!(state.log_buffer, "second");
    }

    // =========================================================================
    // scan_projects — verify projects start unselected
    // =========================================================================

    #[test]
    fn scan_projects_all_start_unselected() {
        let tmp = TempDir::new().expect("create temp dir");
        std::fs::create_dir(tmp.path().join("alpha")).unwrap();
        std::fs::create_dir(tmp.path().join("beta")).unwrap();

        let mut state = make_state();
        state.config.projects_dir = Some(tmp.path().to_string_lossy().into_owned());
        state.scan_projects().expect("scan_projects");
        assert_eq!(state.projects.len(), 2);
        assert!(
            state.projects.iter().all(|p| !p.selected),
            "all scanned projects must start unselected"
        );
    }

    // =========================================================================
    // compute_cluster_memory_limit
    // =========================================================================

    #[test]
    fn compute_cluster_memory_limit_default_80_percent() {
        let state = make_state();
        // Default is 80%. The result should be > 0 on any real machine.
        let limit = state.compute_cluster_memory_limit();
        assert!(limit > 0, "memory limit should be positive, got {}", limit);
    }

    #[test]
    fn compute_cluster_memory_limit_custom_percentage() {
        let mut state = make_state();
        state.config.cluster_memory_percent = 60;
        let limit_60 = state.compute_cluster_memory_limit();

        state.config.cluster_memory_percent = 90;
        let limit_90 = state.compute_cluster_memory_limit();

        assert!(
            limit_90 > limit_60,
            "90% ({}) should be greater than 60% ({})",
            limit_90,
            limit_60
        );
    }

    #[test]
    fn compute_cluster_memory_limit_clamps_low_boundary() {
        let mut state = make_state();
        // Values below 50 are clamped to 50
        state.config.cluster_memory_percent = 10;
        let limit_clamped = state.compute_cluster_memory_limit();

        state.config.cluster_memory_percent = 50;
        let limit_50 = state.compute_cluster_memory_limit();

        assert_eq!(
            limit_clamped, limit_50,
            "10% should be clamped to 50%, got {} vs {}",
            limit_clamped, limit_50
        );
    }

    #[test]
    fn compute_cluster_memory_limit_clamps_high_boundary() {
        let mut state = make_state();
        // Values above 95 are clamped to 95
        state.config.cluster_memory_percent = 100;
        let limit_clamped = state.compute_cluster_memory_limit();

        state.config.cluster_memory_percent = 95;
        let limit_95 = state.compute_cluster_memory_limit();

        assert_eq!(
            limit_clamped, limit_95,
            "100% should be clamped to 95%, got {} vs {}",
            limit_clamped, limit_95
        );
    }

    #[test]
    fn compute_cluster_memory_limit_boundary_50() {
        let mut state = make_state();
        state.config.cluster_memory_percent = 50;
        let limit = state.compute_cluster_memory_limit();
        assert!(limit > 0);
    }

    #[test]
    fn compute_cluster_memory_limit_boundary_95() {
        let mut state = make_state();
        state.config.cluster_memory_percent = 95;
        let limit = state.compute_cluster_memory_limit();
        assert!(limit > 0);
    }

    // =========================================================================
    // write_terraform_vars — platform variations
    // =========================================================================

    #[test]
    fn write_terraform_vars_macos() {
        let tmp = TempDir::new().expect("create temp dir");
        let tf_dir = tmp.path().join("terraform");
        std::fs::create_dir(&tf_dir).unwrap();

        let mut state = make_state();
        state.config.terraform_dir = "terraform".to_string();
        state.project_root = tmp.path().to_path_buf();
        state.platform = Platform::MacOs;

        state.write_terraform_vars().expect("write vars");
        let content = std::fs::read_to_string(tf_dir.join("terraform.auto.tfvars")).unwrap();
        assert!(content.contains("platform = \"macos\""));
        assert!(!content.contains("k3d_volume_mounts"), "macOS should not have k3d volume mounts");
    }

    #[test]
    fn write_terraform_vars_wsl2() {
        let tmp = TempDir::new().expect("create temp dir");
        let tf_dir = tmp.path().join("terraform");
        std::fs::create_dir(&tf_dir).unwrap();

        let mut state = make_state();
        state.config.terraform_dir = "terraform".to_string();
        state.project_root = tmp.path().to_path_buf();
        state.platform = Platform::Wsl2;

        state.write_terraform_vars().expect("write vars");
        let content = std::fs::read_to_string(tf_dir.join("terraform.auto.tfvars")).unwrap();
        assert!(content.contains("platform = \"wsl2\""));
    }

    #[test]
    fn write_terraform_vars_windows_includes_volume_mounts() {
        let tmp = TempDir::new().expect("create temp dir");
        let tf_dir = tmp.path().join("terraform");
        std::fs::create_dir(&tf_dir).unwrap();

        let mut state = make_state();
        state.config.terraform_dir = "terraform".to_string();
        state.project_root = tmp.path().to_path_buf();
        state.platform = Platform::Windows;
        state.config.projects_dir = Some("C:\\Users\\test\\projects".to_string());

        state.write_terraform_vars().expect("write vars");
        let content = std::fs::read_to_string(tf_dir.join("terraform.auto.tfvars")).unwrap();
        assert!(content.contains("platform = \"windows\""));
        assert!(content.contains("k3d_volume_mounts"), "Windows should have k3d volume mounts");
    }

    // =========================================================================
    // selected_projects — all selected
    // =========================================================================

    #[test]
    fn selected_projects_all_selected() {
        let mut state = make_state();
        state.projects = vec![
            make_project("a", true),
            make_project("b", true),
            make_project("c", true),
        ];
        let selected = state.selected_projects();
        assert_eq!(selected.len(), 3);
    }

    // =========================================================================
    // CLU-32: pending_deploy tracks interrupted deployments
    // =========================================================================

    #[test]
    fn pending_deploy_starts_empty() {
        let state = make_state();
        assert!(state.pending_deploy.is_empty());
    }

    #[test]
    fn pending_deploy_tracks_and_clears() {
        let mut state = make_state();
        state.pending_deploy = vec!["proj-a".into(), "proj-b".into()];
        assert_eq!(state.pending_deploy.len(), 2);

        // Simulate successful deploy
        state.pending_deploy.clear();
        assert!(state.pending_deploy.is_empty());
    }

    // =========================================================================
    // APP-13/APP-17: desired_state integration
    // =========================================================================

    #[test]
    fn desired_state_starts_empty() {
        let state = make_state();
        assert!(state.desired_state.deployed_projects.is_empty());
    }

    #[test]
    fn desired_state_tracks_deployments() {
        let mut state = make_state();
        state.desired_state.mark_deployed("proj-a");
        state.desired_state.mark_deployed("proj-b");
        assert_eq!(state.desired_state.deployed_projects.len(), 2);

        state.desired_state.mark_undeployed("proj-a");
        assert_eq!(state.desired_state.deployed_projects.len(), 1);
        assert!(state.desired_state.deployed_projects.contains("proj-b"));
    }

    #[test]
    fn desired_state_find_orphaned() {
        let mut state = make_state();
        state.desired_state.mark_deployed("proj-a");

        let releases = vec![
            "claude-proj-a".to_string(),
            "claude-proj-orphan".to_string(),
        ];
        let orphans = state.desired_state.find_orphaned(&releases);
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0], "claude-proj-orphan");
    }

    #[test]
    fn desired_state_find_missing_deployments() {
        let mut state = make_state();
        state.desired_state.mark_deployed("proj-a");
        state.desired_state.mark_deployed("proj-b");

        let pods = vec!["claude-proj-a-xyz".to_string()];
        let missing = state.desired_state.find_missing_deployments(&pods);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0], "proj-b");
    }

    // UIQ-6: Resource locking tests
    #[test]
    fn resource_lock_acquire_and_release() {
        let mut state = make_state();
        assert!(state.try_lock_resource("proj-a"));
        assert!(state.is_resource_locked("proj-a"));
        assert!(!state.try_lock_resource("proj-a"), "double-lock should fail");
        state.unlock_resource("proj-a");
        assert!(!state.is_resource_locked("proj-a"));
        assert!(state.try_lock_resource("proj-a"), "should re-acquire after release");
    }

    #[test]
    fn resource_lock_independent_resources() {
        let mut state = make_state();
        assert!(state.try_lock_resource("proj-a"));
        assert!(state.try_lock_resource("proj-b"));
        assert!(state.is_resource_locked("proj-a"));
        assert!(state.is_resource_locked("proj-b"));
        state.unlock_resource("proj-a");
        assert!(!state.is_resource_locked("proj-a"));
        assert!(state.is_resource_locked("proj-b"));
    }
}
