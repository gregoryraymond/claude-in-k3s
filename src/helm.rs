use crate::error::{AppResult, CmdResult};
use std::process::Stdio;
use tokio::process::Command;

pub struct HelmRunner {
    binary: String,
    chart_path: String,
    namespace: String,
    release_name: String,
}

impl HelmRunner {
    pub fn new(binary: &str, chart_path: &str, namespace: &str, release_name: &str) -> Self {
        Self {
            binary: binary.into(),
            chart_path: chart_path.into(),
            namespace: namespace.into(),
            release_name: release_name.into(),
        }
    }

    async fn run(&self, args: &[&str]) -> AppResult<CmdResult> {
        let output = Command::new(&self.binary)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        Ok(CmdResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into(),
            stderr: String::from_utf8_lossy(&output.stderr).into(),
        })
    }

    /// Install or upgrade with dynamic project list
    pub async fn install_or_upgrade(
        &self,
        api_key: &str,
        projects: &[(String, String, String)], // (name, path, image)
    ) -> AppResult<CmdResult> {
        let projects_json = serde_json::to_string(
            &projects
                .iter()
                .map(|(name, path, image)| {
                    serde_json::json!({
                        "name": name,
                        "path": path,
                        "image": image,
                    })
                })
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| "[]".into());

        let api_key_set = format!("apiKey={}", api_key);
        let projects_set = format!("projects={}", projects_json);

        self.run(&[
            "upgrade",
            "--install",
            &self.release_name,
            &self.chart_path,
            "--namespace",
            &self.namespace,
            "--create-namespace",
            "--set",
            &api_key_set,
            "--set-json",
            &projects_set,
        ])
        .await
    }

    pub async fn uninstall(&self) -> AppResult<CmdResult> {
        self.run(&[
            "uninstall",
            &self.release_name,
            "--namespace",
            &self.namespace,
        ])
        .await
    }

    pub async fn status(&self) -> AppResult<CmdResult> {
        self.run(&[
            "status",
            &self.release_name,
            "--namespace",
            &self.namespace,
        ])
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_mock_script(path: &std::path::Path, content: &str) {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o755)
            .open(path)
            .unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.sync_all().unwrap();
        drop(f);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    /// Create a mock helm script that exits 0 and prints "ok" to stdout.
    fn mock_helm_success() -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("helm");
        write_mock_script(&script, "#!/bin/bash\necho 'ok'\nexit 0\n");
        (dir, script.to_string_lossy().to_string())
    }

    /// Create a mock helm script that exits 1 and prints an error to stderr.
    fn mock_helm_failure() -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("helm");
        write_mock_script(&script, "#!/bin/bash\necho 'helm command failed' >&2\nexit 1\n");
        (dir, script.to_string_lossy().to_string())
    }

    /// Create a mock helm script that records all arguments to a file, then exits 0.
    fn mock_helm_record_args() -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("helm");
        let args_file = dir.path().join("args");
        write_mock_script(
            &script,
            &format!(
                "#!/bin/bash\necho \"$@\" >> {}\necho 'ok'\nexit 0",
                args_file.display()
            ),
        );
        (dir, script.to_string_lossy().to_string())
    }

    fn make_runner(binary: &str) -> HelmRunner {
        HelmRunner::new(binary, "./helm/claude-code", "claude-code", "my-release")
    }

    #[tokio::test]
    async fn install_or_upgrade_success() {
        let (_dir, binary) = mock_helm_success();
        let runner = make_runner(&binary);
        let projects = vec![(
            "proj1".to_string(),
            "/tmp/proj1".to_string(),
            "img:latest".to_string(),
        )];

        let result = runner.install_or_upgrade("test-key", &projects).await.unwrap();
        assert!(result.success);
        assert_eq!(result.stdout.trim(), "ok");
    }

    #[tokio::test]
    async fn install_or_upgrade_failure() {
        let (_dir, binary) = mock_helm_failure();
        let runner = make_runner(&binary);
        let projects = vec![(
            "proj1".to_string(),
            "/tmp/proj1".to_string(),
            "img:latest".to_string(),
        )];

        let result = runner.install_or_upgrade("test-key", &projects).await.unwrap();
        assert!(!result.success);
        assert!(result.stderr.contains("helm command failed"));
    }

    #[tokio::test]
    async fn install_or_upgrade_passes_correct_args() {
        let (dir, binary) = mock_helm_record_args();
        let runner = make_runner(&binary);
        let projects = vec![(
            "proj1".to_string(),
            "/tmp/proj1".to_string(),
            "img:latest".to_string(),
        )];

        runner.install_or_upgrade("sk-test-123", &projects).await.unwrap();

        let args_file = dir.path().join("args");
        let recorded = fs::read_to_string(&args_file).unwrap();

        assert!(recorded.contains("upgrade --install"), "should contain 'upgrade --install', got: {recorded}");
        assert!(recorded.contains("--namespace"), "should contain '--namespace', got: {recorded}");
        assert!(recorded.contains("--create-namespace"), "should contain '--create-namespace', got: {recorded}");
        assert!(recorded.contains("apiKey="), "should contain 'apiKey=', got: {recorded}");
        assert!(recorded.contains("sk-test-123"), "should contain the api key value, got: {recorded}");
    }

    #[tokio::test]
    async fn install_or_upgrade_serializes_projects_as_json() {
        let (dir, binary) = mock_helm_record_args();
        let runner = make_runner(&binary);
        let projects = vec![
            (
                "alpha".to_string(),
                "/home/user/alpha".to_string(),
                "alpha-img:v1".to_string(),
            ),
            (
                "beta".to_string(),
                "/home/user/beta".to_string(),
                "beta-img:v2".to_string(),
            ),
        ];

        runner.install_or_upgrade("key-abc", &projects).await.unwrap();

        let args_file = dir.path().join("args");
        let recorded = fs::read_to_string(&args_file).unwrap();

        assert!(recorded.contains("--set-json"), "should contain '--set-json', got: {recorded}");
        assert!(recorded.contains("alpha"), "should contain project name 'alpha', got: {recorded}");
        assert!(recorded.contains("beta"), "should contain project name 'beta', got: {recorded}");
    }

    #[tokio::test]
    async fn install_with_empty_projects() {
        let (dir, binary) = mock_helm_record_args();
        let runner = make_runner(&binary);
        let projects: Vec<(String, String, String)> = vec![];

        runner.install_or_upgrade("key-empty", &projects).await.unwrap();

        let args_file = dir.path().join("args");
        let recorded = fs::read_to_string(&args_file).unwrap();

        assert!(recorded.contains("projects=[]"), "should contain 'projects=[]', got: {recorded}");
    }

    #[tokio::test]
    async fn uninstall_success() {
        let (_dir, binary) = mock_helm_success();
        let runner = make_runner(&binary);

        let result = runner.uninstall().await.unwrap();
        assert!(result.success);
        assert_eq!(result.stdout.trim(), "ok");
    }

    #[tokio::test]
    async fn uninstall_passes_correct_args() {
        let (dir, binary) = mock_helm_record_args();
        let runner = make_runner(&binary);

        runner.uninstall().await.unwrap();

        let args_file = dir.path().join("args");
        let recorded = fs::read_to_string(&args_file).unwrap();

        assert!(
            recorded.contains("uninstall my-release --namespace claude-code"),
            "should contain 'uninstall my-release --namespace claude-code', got: {recorded}"
        );
    }

    #[tokio::test]
    async fn status_success() {
        let (_dir, binary) = mock_helm_success();
        let runner = make_runner(&binary);

        let result = runner.status().await.unwrap();
        assert!(result.success);
        assert_eq!(result.stdout.trim(), "ok");
    }
}
