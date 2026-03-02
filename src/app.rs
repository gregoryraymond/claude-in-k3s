use crate::config::AppConfig;
use crate::deps::{self, DepsStatus};
use crate::docker::DockerBuilder;
use crate::error::AppResult;
use crate::helm::HelmRunner;
use crate::kubectl::{KubectlRunner, PodStatus};
use crate::platform::{self, Platform};
use crate::projects::{self, Project};
use crate::terraform::TerraformRunner;
use std::path::{Path, PathBuf};

/// Shared application state
pub struct AppState {
    pub config: AppConfig,
    pub platform: Platform,
    pub projects: Vec<Project>,
    pub pods: Vec<PodStatus>,
    pub cluster_healthy: bool,
    pub tf_initialized: bool,
    #[allow(dead_code)] // used by upcoming SetupPanel UI wiring
    pub deps_status: DepsStatus,
    pub log_buffer: String,
    project_root: PathBuf,
}

impl AppState {
    pub fn new() -> AppResult<Self> {
        let config = AppConfig::load()?;
        let plat = platform::detect_platform();
        let deps_status = deps::check_all(&plat);

        let project_root = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        // Try to find the project root by looking for Cargo.toml
        let project_root = find_project_root().unwrap_or(project_root);

        Ok(Self {
            config,
            platform: plat,
            projects: vec![],
            pods: vec![],
            cluster_healthy: false,
            tf_initialized: false,
            deps_status,
            log_buffer: String::new(),
            project_root,
        })
    }

    pub fn append_log(&mut self, msg: &str) {
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

    pub fn terraform_runner(&self) -> TerraformRunner {
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
            },
            log_buffer: String::new(),
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
}
