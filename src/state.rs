//! APP-13/APP-17: Persistent desired-state record.
//!
//! Tracks which projects the user has deployed so the app can reconcile
//! after restart (APP-4) and detect orphaned deployments (APP-5).

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Desired state persisted across app restarts.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct DesiredState {
    /// Project names the user has explicitly deployed.
    #[serde(default)]
    pub deployed_projects: BTreeSet<String>,
}

impl DesiredState {
    /// Path to the state file under the app config directory.
    pub fn state_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("claude-in-k3s")
            .join("state.json")
    }

    /// Load state from disk. Returns default if file missing or corrupt.
    pub fn load() -> Self {
        Self::load_from(&Self::state_path())
    }

    /// Load from an arbitrary path (for testing).
    pub fn load_from(path: &std::path::Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist state to disk.
    pub fn save(&self) -> Result<(), String> {
        self.save_to(&Self::state_path())
    }

    /// Save to an arbitrary path (for testing).
    pub fn save_to(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create state dir: {}", e))?;
        }
        let content =
            serde_json::to_string_pretty(self).map_err(|e| format!("Serialize error: {}", e))?;
        std::fs::write(path, content).map_err(|e| format!("Write error: {}", e))
    }

    /// Record a project as deployed.
    pub fn mark_deployed(&mut self, project_name: &str) {
        self.deployed_projects.insert(project_name.to_string());
    }

    /// Remove a project from desired state (after delete / undeploy).
    pub fn mark_undeployed(&mut self, project_name: &str) {
        self.deployed_projects.remove(project_name);
    }

    /// APP-5: Detect orphaned deployments — helm releases that are not in the desired state.
    pub fn find_orphaned<'a>(&self, helm_releases: &'a [String]) -> Vec<&'a String> {
        helm_releases
            .iter()
            .filter(|release| {
                // Helm releases are named "claude-<project>", extract the project name
                if let Some(project) = release.strip_prefix("claude-") {
                    !self.deployed_projects.contains(project)
                } else {
                    // Non-claude releases are not ours to manage
                    false
                }
            })
            .collect()
    }

    /// APP-4: Find projects that should be deployed but have no running pod.
    pub fn find_missing_deployments<'a>(&'a self, running_pod_names: &[String]) -> Vec<&'a String> {
        self.deployed_projects
            .iter()
            .filter(|proj| !running_pod_names.iter().any(|pod| pod.contains(proj.as_str())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_state_is_empty() {
        let state = DesiredState::default();
        assert!(state.deployed_projects.is_empty());
    }

    #[test]
    fn mark_deployed_adds_project() {
        let mut state = DesiredState::default();
        state.mark_deployed("my-project");
        assert!(state.deployed_projects.contains("my-project"));
        assert_eq!(state.deployed_projects.len(), 1);
    }

    #[test]
    fn mark_deployed_idempotent() {
        let mut state = DesiredState::default();
        state.mark_deployed("proj-a");
        state.mark_deployed("proj-a");
        assert_eq!(state.deployed_projects.len(), 1);
    }

    #[test]
    fn mark_undeployed_removes_project() {
        let mut state = DesiredState::default();
        state.mark_deployed("proj-a");
        state.mark_deployed("proj-b");
        state.mark_undeployed("proj-a");
        assert!(!state.deployed_projects.contains("proj-a"));
        assert!(state.deployed_projects.contains("proj-b"));
    }

    #[test]
    fn mark_undeployed_nonexistent_is_noop() {
        let mut state = DesiredState::default();
        state.mark_undeployed("nonexistent");
        assert!(state.deployed_projects.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.json");

        let mut state = DesiredState::default();
        state.mark_deployed("alpha");
        state.mark_deployed("beta");

        state.save_to(&path).expect("save");
        let loaded = DesiredState::load_from(&path);
        assert_eq!(loaded, state);
    }

    #[test]
    fn load_nonexistent_returns_default() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("no-such-file.json");
        let state = DesiredState::load_from(&path);
        assert!(state.deployed_projects.is_empty());
    }

    #[test]
    fn load_corrupt_returns_default() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.json");
        std::fs::write(&path, "not valid json {{{").unwrap();
        let state = DesiredState::load_from(&path);
        assert!(state.deployed_projects.is_empty());
    }

    #[test]
    fn load_empty_file_returns_default() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("state.json");
        std::fs::write(&path, "").unwrap();
        let state = DesiredState::load_from(&path);
        assert!(state.deployed_projects.is_empty());
    }

    #[test]
    fn save_creates_parent_dirs() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a").join("b").join("state.json");
        let state = DesiredState::default();
        state.save_to(&path).expect("save should create dirs");
        assert!(path.exists());
    }

    #[test]
    fn state_path_under_config_dir() {
        let path = DesiredState::state_path();
        assert!(path.to_string_lossy().contains("claude-in-k3s"));
        assert!(path.to_string_lossy().ends_with("state.json"));
    }

    #[test]
    fn find_orphaned_empty() {
        let state = DesiredState::default();
        let releases: Vec<String> = vec![];
        assert!(state.find_orphaned(&releases).is_empty());
    }

    #[test]
    fn find_orphaned_detects_unknown_releases() {
        let mut state = DesiredState::default();
        state.mark_deployed("proj-a");

        let releases = vec![
            "claude-proj-a".to_string(),
            "claude-proj-b".to_string(), // orphan
            "claude-proj-c".to_string(), // orphan
        ];
        let orphans = state.find_orphaned(&releases);
        assert_eq!(orphans.len(), 2);
        assert!(orphans.contains(&&"claude-proj-b".to_string()));
        assert!(orphans.contains(&&"claude-proj-c".to_string()));
    }

    #[test]
    fn find_orphaned_ignores_non_claude_releases() {
        let state = DesiredState::default();
        let releases = vec!["some-other-release".to_string()];
        assert!(state.find_orphaned(&releases).is_empty());
    }

    #[test]
    fn find_orphaned_no_orphans() {
        let mut state = DesiredState::default();
        state.mark_deployed("proj-a");
        state.mark_deployed("proj-b");

        let releases = vec![
            "claude-proj-a".to_string(),
            "claude-proj-b".to_string(),
        ];
        assert!(state.find_orphaned(&releases).is_empty());
    }

    #[test]
    fn find_missing_deployments_empty() {
        let state = DesiredState::default();
        let pods: Vec<String> = vec![];
        assert!(state.find_missing_deployments(&pods).is_empty());
    }

    #[test]
    fn find_missing_deployments_detects_missing() {
        let mut state = DesiredState::default();
        state.mark_deployed("proj-a");
        state.mark_deployed("proj-b");

        let pods = vec!["claude-proj-a-xyz123".to_string()];
        let missing = state.find_missing_deployments(&pods);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0], "proj-b");
    }

    #[test]
    fn find_missing_deployments_all_running() {
        let mut state = DesiredState::default();
        state.mark_deployed("proj-a");

        let pods = vec!["claude-proj-a-abc123".to_string()];
        assert!(state.find_missing_deployments(&pods).is_empty());
    }

    #[test]
    fn deployed_projects_sorted() {
        let mut state = DesiredState::default();
        state.mark_deployed("zebra");
        state.mark_deployed("alpha");
        state.mark_deployed("middle");

        let names: Vec<&String> = state.deployed_projects.iter().collect();
        assert_eq!(names, vec!["alpha", "middle", "zebra"]);
    }
}
