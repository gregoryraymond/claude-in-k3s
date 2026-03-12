use crate::error::{AppResult, CmdResult};
use std::process::Stdio;
use tokio::process::Command;

/// Abstraction over Helm operations for testability.
///
/// Concrete implementation: [`HelmRunner`].
pub trait HelmOps: Send + Sync {
    /// Install or upgrade a single project as its own helm release.
    fn install_project(
        &self,
        project_name: &str,
        project_path: &str,
        project_image: &str,
        extra_set_args: &[(&str, &str)],
    ) -> impl std::future::Future<Output = AppResult<CmdResult>> + Send;

    /// Uninstall a specific project's helm release.
    fn uninstall_project(
        &self,
        project_name: &str,
    ) -> impl std::future::Future<Output = AppResult<CmdResult>> + Send;

    /// Uninstall by release name directly.
    fn uninstall(
        &self,
        release_name: &str,
    ) -> impl std::future::Future<Output = AppResult<CmdResult>> + Send;

    /// List all releases in the namespace.
    fn list_releases(
        &self,
    ) -> impl std::future::Future<Output = AppResult<CmdResult>> + Send;

    /// Count the number of active helm releases.
    fn release_count(&self) -> impl std::future::Future<Output = usize> + Send;

    /// Check status of the overall namespace.
    fn status(
        &self,
    ) -> impl std::future::Future<Output = AppResult<CmdResult>> + Send;
}

pub struct HelmRunner {
    binary: String,
    chart_path: String,
    namespace: String,
}

impl HelmRunner {
    pub fn new(binary: &str, chart_path: &str, namespace: &str) -> Self {
        Self {
            binary: binary.into(),
            chart_path: chart_path.into(),
            namespace: namespace.into(),
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

    /// Release name for a project: "claude-<sanitized-name>"
    /// Replaces all non-alphanumeric characters with `-`, collapses runs.
    /// Public accessor for use outside the struct.
    pub fn release_name_for(project_name: &str) -> String {
        Self::release_name(project_name)
    }

    fn release_name(project_name: &str) -> String {
        let sanitized: String = project_name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
            .collect();
        // Collapse consecutive dashes and trim trailing dashes
        let collapsed: String = sanitized
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let truncated = if collapsed.len() > 53 {
            &collapsed[..53]
        } else {
            &collapsed
        };
        format!("claude-{}", truncated.trim_end_matches('-'))
    }

    /// PRJ-60: Given a list of project names, return a mapping of each name to
    /// a unique release name. If two projects sanitize to the same name, a numeric
    /// suffix is appended (e.g., `claude-my-app`, `claude-my-app-2`).
    pub fn deduplicated_release_names(project_names: &[&str]) -> Vec<(String, String)> {
        use std::collections::HashMap;
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut result = Vec::new();
        for &name in project_names {
            let base = Self::release_name(name);
            let count = counts.entry(base.clone()).or_insert(0);
            *count += 1;
            let release = if *count == 1 {
                base
            } else {
                format!("{}-{}", base, count)
            };
            result.push((name.to_string(), release));
        }
        result
    }

    /// Install or upgrade a single project as its own helm release.
    pub async fn install_project(
        &self,
        project_name: &str,
        project_path: &str,
        project_image: &str,
        extra_set_args: &[(&str, &str)],
    ) -> AppResult<CmdResult> {
        let release = Self::release_name(project_name);

        let mut args = vec![
            "upgrade",
            "--install",
            &release,
            &self.chart_path,
            "--namespace",
            &self.namespace,
            "--create-namespace",
            "--set",
        ];

        let name_val = format!("project.name={}", project_name);
        args.push(&name_val);
        args.push("--set");
        let path_val = format!("project.path={}", project_path);
        args.push(&path_val);
        args.push("--set");
        let image_val = format!("project.image={}", project_image);
        args.push(&image_val);

        let extra_formatted: Vec<String> = extra_set_args
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        for extra in &extra_formatted {
            args.push("--set");
            args.push(extra);
        }

        self.run(&args).await
    }

    /// Uninstall a specific project's helm release.
    pub async fn uninstall_project(&self, project_name: &str) -> AppResult<CmdResult> {
        let release = Self::release_name(project_name);
        self.run(&[
            "uninstall",
            &release,
            "--namespace",
            &self.namespace,
        ])
        .await
    }

    /// Uninstall by release name directly (for legacy or recovery).
    pub async fn uninstall(&self, release_name: &str) -> AppResult<CmdResult> {
        self.run(&[
            "uninstall",
            release_name,
            "--namespace",
            &self.namespace,
        ])
        .await
    }

    /// Check status of a specific project's release.
    #[allow(dead_code)]
    pub async fn status_project(&self, project_name: &str) -> AppResult<CmdResult> {
        let release = Self::release_name(project_name);
        self.run(&[
            "status",
            &release,
            "--namespace",
            &self.namespace,
        ])
        .await
    }

    /// List all releases in the namespace.
    pub async fn list_releases(&self) -> AppResult<CmdResult> {
        self.run(&[
            "list",
            "--namespace",
            &self.namespace,
            "--short",
        ])
        .await
    }

    /// Check status of the overall namespace (any release).
    pub async fn status(&self) -> AppResult<CmdResult> {
        // Check if any releases exist
        self.list_releases().await
    }

    /// Count the number of active helm releases in the namespace.
    pub async fn release_count(&self) -> usize {
        match self.list_releases().await {
            Ok(r) if r.success => {
                r.stdout.lines().filter(|l| !l.trim().is_empty()).count()
            }
            _ => 0,
        }
    }
}

impl HelmOps for HelmRunner {
    async fn install_project(
        &self,
        project_name: &str,
        project_path: &str,
        project_image: &str,
        extra_set_args: &[(&str, &str)],
    ) -> AppResult<CmdResult> {
        HelmRunner::install_project(self, project_name, project_path, project_image, extra_set_args)
            .await
    }

    async fn uninstall_project(&self, project_name: &str) -> AppResult<CmdResult> {
        HelmRunner::uninstall_project(self, project_name).await
    }

    async fn uninstall(&self, release_name: &str) -> AppResult<CmdResult> {
        HelmRunner::uninstall(self, release_name).await
    }

    async fn list_releases(&self) -> AppResult<CmdResult> {
        HelmRunner::list_releases(self).await
    }

    async fn release_count(&self) -> usize {
        HelmRunner::release_count(self).await
    }

    async fn status(&self) -> AppResult<CmdResult> {
        HelmRunner::status(self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_name_simple() {
        assert_eq!(HelmRunner::release_name("my-project"), "claude-my-project");
    }

    #[test]
    fn release_name_underscores() {
        assert_eq!(HelmRunner::release_name("my_project"), "claude-my-project");
    }

    #[test]
    fn release_name_dots() {
        assert_eq!(HelmRunner::release_name("my.project"), "claude-my-project");
    }

    #[test]
    fn release_name_uppercase() {
        assert_eq!(HelmRunner::release_name("MyProject"), "claude-myproject");
    }

    #[test]
    fn release_name_special_chars() {
        // PRJ-59: all non-alphanumeric chars should be replaced
        assert_eq!(HelmRunner::release_name("my@project!"), "claude-my-project");
        assert_eq!(HelmRunner::release_name("my project"), "claude-my-project");
        assert_eq!(HelmRunner::release_name("a++b"), "claude-a-b");
        assert_eq!(HelmRunner::release_name("hello___world"), "claude-hello-world");
        assert_eq!(HelmRunner::release_name("a..b..c"), "claude-a-b-c");
    }

    #[test]
    fn release_name_no_trailing_dash() {
        assert_eq!(HelmRunner::release_name("test-"), "claude-test");
        assert_eq!(HelmRunner::release_name("test---"), "claude-test");
    }

    #[test]
    fn release_name_truncation() {
        let long_name = "a".repeat(60);
        let release = HelmRunner::release_name(&long_name);
        // "claude-" is 7 chars + 53 = 60, under k8s 63 char limit
        assert!(release.len() <= 63);
        assert!(release.starts_with("claude-"));
    }

    #[test]
    fn deduplicated_no_conflicts() {
        let names = vec!["alpha", "beta", "gamma"];
        let result = HelmRunner::deduplicated_release_names(&names);
        assert_eq!(result[0], ("alpha".into(), "claude-alpha".into()));
        assert_eq!(result[1], ("beta".into(), "claude-beta".into()));
        assert_eq!(result[2], ("gamma".into(), "claude-gamma".into()));
    }

    #[test]
    fn deduplicated_with_conflicts() {
        // "my_project" and "my.project" both sanitize to "claude-my-project"
        let names = vec!["my_project", "my.project"];
        let result = HelmRunner::deduplicated_release_names(&names);
        assert_eq!(result[0].1, "claude-my-project");
        assert_eq!(result[1].1, "claude-my-project-2");
    }

    #[test]
    fn deduplicated_triple_conflict() {
        let names = vec!["my_project", "my.project", "my project"];
        let result = HelmRunner::deduplicated_release_names(&names);
        assert_eq!(result[0].1, "claude-my-project");
        assert_eq!(result[1].1, "claude-my-project-2");
        assert_eq!(result[2].1, "claude-my-project-3");
    }
}

#[cfg(all(test, unix))]
mod integration_tests {
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

    fn mock_helm_success() -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("helm");
        write_mock_script(&script, "#!/bin/bash\necho 'ok'\nexit 0\n");
        (dir, script.to_string_lossy().to_string())
    }

    fn mock_helm_failure() -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("helm");
        write_mock_script(&script, "#!/bin/bash\necho 'helm command failed' >&2\nexit 1\n");
        (dir, script.to_string_lossy().to_string())
    }

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
        HelmRunner::new(binary, "./helm/claude-code", "claude-code")
    }

    #[tokio::test]
    async fn install_project_success() {
        let (_dir, binary) = mock_helm_success();
        let runner = make_runner(&binary);

        let result = runner
            .install_project("my-project", "/tmp/project", "img:latest", &[])
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.stdout.trim(), "ok");
    }

    #[tokio::test]
    async fn install_project_failure() {
        let (_dir, binary) = mock_helm_failure();
        let runner = make_runner(&binary);

        let result = runner
            .install_project("my-project", "/tmp/project", "img:latest", &[])
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.stderr.contains("helm command failed"));
    }

    #[tokio::test]
    async fn install_project_passes_correct_args() {
        let (dir, binary) = mock_helm_record_args();
        let runner = make_runner(&binary);

        runner
            .install_project(
                "my-project",
                "/tmp/project",
                "img:latest",
                &[("claude.credentialsPath", "/home/user/.claude")],
            )
            .await
            .unwrap();

        let args_file = dir.path().join("args");
        let recorded = fs::read_to_string(&args_file).unwrap();

        assert!(recorded.contains("upgrade --install claude-my-project"), "got: {recorded}");
        assert!(recorded.contains("--namespace claude-code"), "got: {recorded}");
        assert!(recorded.contains("project.name=my-project"), "got: {recorded}");
        assert!(recorded.contains("project.path=/tmp/project"), "got: {recorded}");
        assert!(recorded.contains("project.image=img:latest"), "got: {recorded}");
        assert!(recorded.contains("claude.credentialsPath="), "got: {recorded}");
    }

    #[tokio::test]
    async fn uninstall_project_passes_correct_args() {
        let (dir, binary) = mock_helm_record_args();
        let runner = make_runner(&binary);

        runner.uninstall_project("my-project").await.unwrap();

        let args_file = dir.path().join("args");
        let recorded = fs::read_to_string(&args_file).unwrap();

        assert!(
            recorded.contains("uninstall claude-my-project --namespace claude-code"),
            "got: {recorded}"
        );
    }

    #[tokio::test]
    async fn status_project_success() {
        let (_dir, binary) = mock_helm_success();
        let runner = make_runner(&binary);

        let result = runner.status_project("my-project").await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn list_releases_success() {
        let (_dir, binary) = mock_helm_success();
        let runner = make_runner(&binary);

        let result = runner.list_releases().await.unwrap();
        assert!(result.success);
    }
}

#[cfg(test)]
mod cross_platform_integration_tests {
    use super::*;
    use crate::test_utils::{create_mock_binary, create_mock_binary_recording, create_mock_binary_with_stderr};
    use tempfile::TempDir;

    fn make_runner(binary: &str) -> HelmRunner {
        HelmRunner::new(binary, "./helm/claude-code", "claude-code")
    }

    #[tokio::test]
    async fn helm_install_calls_upgrade_install() {
        let dir = TempDir::new().unwrap();
        let (mock_path, args_file) =
            create_mock_binary_recording(dir.path(), "helm", "ok", 0);

        let runner = make_runner(mock_path.to_str().unwrap());
        let result = runner
            .install_project("test-proj", "/tmp/proj", "img:latest", &[])
            .await
            .unwrap();

        assert!(result.success);
        let recorded = std::fs::read_to_string(&args_file).unwrap();
        assert!(
            recorded.contains("upgrade --install"),
            "expected 'upgrade --install' in recorded args: {recorded}"
        );
    }

    #[tokio::test]
    async fn helm_uninstall_calls_correct_release() {
        let dir = TempDir::new().unwrap();
        let (mock_path, args_file) =
            create_mock_binary_recording(dir.path(), "helm", "ok", 0);

        let runner = make_runner(mock_path.to_str().unwrap());
        runner.uninstall_project("my-project").await.unwrap();

        let recorded = std::fs::read_to_string(&args_file).unwrap();
        assert!(
            recorded.contains("uninstall claude-my-project"),
            "expected 'uninstall claude-my-project' in: {recorded}"
        );
        assert!(
            recorded.contains("--namespace claude-code"),
            "expected '--namespace claude-code' in: {recorded}"
        );
    }

    #[tokio::test]
    async fn helm_status_returns_output() {
        let dir = TempDir::new().unwrap();
        let mock_path = create_mock_binary(dir.path(), "helm", "STATUS: deployed", 0);

        let runner = make_runner(mock_path.to_str().unwrap());
        let result = runner.status_project("my-project").await.unwrap();

        assert!(result.success);
        assert!(
            result.stdout.contains("STATUS: deployed"),
            "expected stdout to contain 'STATUS: deployed', got: {}",
            result.stdout
        );
    }

    #[tokio::test]
    async fn helm_list_returns_releases() {
        let dir = TempDir::new().unwrap();
        let mock_path =
            create_mock_binary(dir.path(), "helm", "claude-proj-a\nclaude-proj-b", 0);

        let runner = make_runner(mock_path.to_str().unwrap());
        let result = runner.list_releases().await.unwrap();

        assert!(result.success);
        let lines: Vec<&str> = result.stdout.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 2, "expected 2 releases, got: {:?}", lines);
        assert!(lines[0].contains("claude-proj-a"));
        assert!(lines[1].contains("claude-proj-b"));
    }

    #[tokio::test]
    async fn helm_install_failure_returns_stderr() {
        let dir = TempDir::new().unwrap();
        let mock_path = create_mock_binary_with_stderr(
            dir.path(),
            "helm",
            "",
            "Error: release failed",
            1,
        );

        let runner = make_runner(mock_path.to_str().unwrap());
        let result = runner
            .install_project("fail-proj", "/tmp/p", "img:latest", &[])
            .await
            .unwrap();

        assert!(!result.success);
        assert!(
            result.stderr.contains("release failed"),
            "expected stderr to contain 'release failed', got: {}",
            result.stderr
        );
    }
}
