use crate::error::{AppError, AppResult, CmdResult};
use crate::projects::{BaseImage, Project};
use std::process::Stdio;
use tokio::process::Command;

pub struct DockerBuilder {
    docker_binary: String,
    template_dir: String,
}

impl DockerBuilder {
    pub fn new(docker_binary: &str, template_dir: &str) -> Self {
        Self {
            docker_binary: docker_binary.into(),
            template_dir: template_dir.into(),
        }
    }

    /// Build a Docker image for a project using the template Dockerfile
    pub async fn build_preset(
        &self,
        base_image: &BaseImage,
        tag: &str,
    ) -> AppResult<CmdResult> {
        let output = Command::new(&self.docker_binary)
            .args([
                "build",
                "-t",
                tag,
                "--build-arg",
                &format!("BASE_IMAGE={}", base_image.docker_image()),
                "-f",
                &format!("{}/Dockerfile.template", self.template_dir),
                &self.template_dir,
            ])
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

    /// Build a Docker image from a project's custom Dockerfile
    pub async fn build_custom(
        &self,
        project: &Project,
        tag: &str,
    ) -> AppResult<CmdResult> {
        let dockerfile = if project.path.join(".claude").join("Dockerfile").exists() {
            project.path.join(".claude").join("Dockerfile")
        } else if project.path.join("Dockerfile").exists() {
            project.path.join("Dockerfile")
        } else {
            return Err(AppError::Docker(format!(
                "No Dockerfile found in project '{}'",
                project.name
            )));
        };

        let output = Command::new(&self.docker_binary)
            .args([
                "build",
                "-t",
                tag,
                "-f",
                &dockerfile.to_string_lossy(),
                &project.path.to_string_lossy(),
            ])
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

    /// Import a Docker image into k3s containerd
    pub async fn import_to_k3s(&self, tag: &str) -> AppResult<CmdResult> {
        // docker save <tag> | sudo k3s ctr images import -
        let mut save = Command::new(&self.docker_binary)
            .args(["save", tag])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let save_stdout = save
            .stdout
            .take()
            .ok_or_else(|| AppError::Docker("Failed to capture docker save stdout".into()))?;

        // Convert tokio ChildStdout to std Stdio via the owned handle/fd
        #[cfg(windows)]
        let std_stdio: Stdio = save_stdout.into_owned_handle()?.into();
        #[cfg(unix)]
        let std_stdio: Stdio = save_stdout.into_owned_fd()?.into();

        let output = Command::new("sudo")
            .args(["k3s", "ctr", "images", "import", "-"])
            .stdin(std_stdio)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        // Wait for the save process to finish
        let _ = save.wait().await;

        Ok(CmdResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into(),
            stderr: String::from_utf8_lossy(&output.stderr).into(),
        })
    }

    /// Build and import a project's image
    pub async fn build_and_import(&self, project: &Project) -> AppResult<CmdResult> {
        let tag = image_tag_for_project(project);

        let build_result = if project.base_image == BaseImage::Custom {
            self.build_custom(project, &tag).await?
        } else {
            self.build_preset(&project.base_image, &tag).await?
        };

        if !build_result.success {
            return Ok(build_result);
        }

        self.import_to_k3s(&tag).await
    }
}

/// Generate a deterministic image tag for a project
pub fn image_tag_for_project(project: &Project) -> String {
    let sanitized = project
        .name
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-', "-");
    format!("claude-code-{}:latest", sanitized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// Helper: create a Project with the given name and defaults
    fn make_project(name: &str) -> Project {
        Project {
            name: name.to_string(),
            path: PathBuf::from("/tmp/fake"),
            selected: false,
            base_image: BaseImage::Node,
            has_custom_dockerfile: false,
        }
    }

    /// Helper: create a Project with a real path
    fn make_project_with_path(name: &str, path: PathBuf) -> Project {
        Project {
            name: name.to_string(),
            path,
            selected: false,
            base_image: BaseImage::Custom,
            has_custom_dockerfile: true,
        }
    }

    // ── image_tag_for_project tests ──────────────────────────────────

    #[test]
    fn image_tag_simple_name() {
        let p = make_project("my-project");
        assert_eq!(image_tag_for_project(&p), "claude-code-my-project:latest");
    }

    #[test]
    fn image_tag_uppercase_lowered() {
        let p = make_project("MyProject");
        assert_eq!(image_tag_for_project(&p), "claude-code-myproject:latest");
    }

    #[test]
    fn image_tag_special_chars_replaced() {
        let p = make_project("my.project_v2");
        assert_eq!(
            image_tag_for_project(&p),
            "claude-code-my-project-v2:latest"
        );
    }

    #[test]
    fn image_tag_spaces_replaced() {
        let p = make_project("my project");
        assert_eq!(image_tag_for_project(&p), "claude-code-my-project:latest");
    }

    #[test]
    fn image_tag_already_valid() {
        let p = make_project("valid-name");
        assert_eq!(image_tag_for_project(&p), "claude-code-valid-name:latest");
    }

    #[test]
    fn image_tag_numeric() {
        let p = make_project("12345");
        assert_eq!(image_tag_for_project(&p), "claude-code-12345:latest");
    }

    // ── DockerBuilder construction ───────────────────────────────────

    #[test]
    fn docker_builder_construction() {
        let builder = DockerBuilder::new("/usr/bin/docker", "/opt/templates");
        assert_eq!(builder.docker_binary, "/usr/bin/docker");
        assert_eq!(builder.template_dir, "/opt/templates");
    }

    // ── build_custom: no Dockerfile → error ──────────────────────────

    #[tokio::test]
    async fn build_custom_no_dockerfile_returns_error() {
        let tmp = TempDir::new().unwrap();
        let project = make_project_with_path("test-proj", tmp.path().to_path_buf());

        let builder = DockerBuilder::new("docker", "/tmp/templates");
        let result = builder.build_custom(&project, "test:latest").await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("No Dockerfile found"),
            "expected 'No Dockerfile found' in: {msg}"
        );
        assert!(
            msg.contains("test-proj"),
            "expected project name in: {msg}"
        );
        // Verify it is specifically a Docker variant
        assert!(
            matches!(err, AppError::Docker(_)),
            "expected AppError::Docker, got: {err:?}"
        );
    }

    // ── build_preset with mock docker binary ─────────────────────────

    fn write_mock_script(path: &std::path::Path, content: &str) {
        use std::io::Write;
        #[cfg(unix)]
        use std::os::unix::fs::OpenOptionsExt;
        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).write(true).truncate(true);
        #[cfg(unix)]
        opts.mode(0o755);
        let mut f = opts.open(path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.sync_all().unwrap();
        drop(f);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    #[tokio::test]
    async fn build_preset_with_mock() {
        let mock_dir = TempDir::new().unwrap();
        let mock_path = mock_dir.path().join("docker");
        write_mock_script(&mock_path, "#!/bin/bash\necho 'build-output'\nexit 0");

        // Also create a dummy Dockerfile.template so the arg is valid
        let template_dir = TempDir::new().unwrap();
        std::fs::write(
            template_dir.path().join("Dockerfile.template"),
            "FROM scratch\n",
        )
        .unwrap();

        let builder = DockerBuilder::new(
            mock_path.to_str().unwrap(),
            template_dir.path().to_str().unwrap(),
        );

        let result = builder
            .build_preset(&BaseImage::Node, "test-image:latest")
            .await
            .expect("build_preset should succeed");

        assert!(result.success);
        assert!(
            result.stdout.contains("build-output"),
            "expected stdout to contain 'build-output', got: {}",
            result.stdout
        );
    }

    #[tokio::test]
    async fn build_preset_failure_with_mock() {
        let mock_dir = TempDir::new().unwrap();
        let mock_path = mock_dir.path().join("docker");
        write_mock_script(&mock_path, "#!/bin/bash\necho 'build failed' >&2\nexit 1");

        let template_dir = TempDir::new().unwrap();
        std::fs::write(
            template_dir.path().join("Dockerfile.template"),
            "FROM scratch\n",
        )
        .unwrap();

        let builder = DockerBuilder::new(
            mock_path.to_str().unwrap(),
            template_dir.path().to_str().unwrap(),
        );

        let result = builder
            .build_preset(&BaseImage::Python, "fail-image:latest")
            .await
            .expect("build_preset should return Ok even on docker failure");

        assert!(!result.success);
        assert!(
            result.stderr.contains("build failed"),
            "expected stderr to contain 'build failed', got: {}",
            result.stderr
        );
    }

    // ── build_and_import: skips import when build fails ──────────────

    #[tokio::test]
    async fn build_and_import_skips_import_on_build_failure() {
        // The mock docker binary always fails with exit 1.
        // build_and_import should return the failed CmdResult without
        // attempting import_to_k3s (which would also fail because sudo/k3s
        // are not available in CI).
        let mock_dir = TempDir::new().unwrap();
        let mock_path = mock_dir.path().join("docker");
        write_mock_script(&mock_path, "#!/bin/bash\necho 'build-error' >&2\nexit 1");

        let template_dir = TempDir::new().unwrap();
        std::fs::write(
            template_dir.path().join("Dockerfile.template"),
            "FROM scratch\n",
        )
        .unwrap();

        let builder = DockerBuilder::new(
            mock_path.to_str().unwrap(),
            template_dir.path().to_str().unwrap(),
        );

        // Use a non-Custom base_image so build_and_import takes the
        // build_preset path (avoids needing a real project directory with
        // a Dockerfile).
        let project = Project {
            name: "fail-proj".to_string(),
            path: PathBuf::from("/tmp/nonexistent"),
            selected: false,
            base_image: BaseImage::Node,
            has_custom_dockerfile: false,
        };

        let result = builder
            .build_and_import(&project)
            .await
            .expect("build_and_import should return Ok with failed CmdResult");

        assert!(!result.success, "build should have failed");
        assert!(
            result.stderr.contains("build-error"),
            "expected stderr to contain 'build-error', got: {}",
            result.stderr
        );
    }
}
