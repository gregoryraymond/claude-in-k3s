use crate::error::{AppResult, CmdResult};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

pub struct TerraformRunner {
    binary: String,
    working_dir: String,
}

impl TerraformRunner {
    pub fn new(binary: &str, working_dir: &str) -> Self {
        Self {
            binary: binary.into(),
            working_dir: working_dir.into(),
        }
    }

    async fn run(&self, args: &[&str]) -> AppResult<CmdResult> {
        let output = Command::new(&self.binary)
            .args(args)
            .current_dir(&self.working_dir)
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

    pub async fn init(&self) -> AppResult<CmdResult> {
        self.run(&["init", "-no-color"]).await
    }

    pub async fn apply(&self) -> AppResult<CmdResult> {
        self.run(&["apply", "-auto-approve", "-no-color"]).await
    }

    pub async fn destroy(&self) -> AppResult<CmdResult> {
        self.run(&["destroy", "-auto-approve", "-no-color"]).await
    }

    pub async fn plan(&self) -> AppResult<CmdResult> {
        self.run(&["plan", "-no-color"]).await
    }

    pub fn is_initialized(&self) -> bool {
        Path::new(&self.working_dir).join(".terraform").exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn mock_terraform(body: &str) -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("terraform");
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(0o755)
                .open(&script)
                .unwrap();
            f.write_all(format!("#!/bin/bash\n{}", body).as_bytes()).unwrap();
            f.sync_all().unwrap();
        }
        // Brief sleep to let the kernel finalize the close; avoids ETXTBSY
        // when many tests create and execute scripts in parallel.
        std::thread::sleep(std::time::Duration::from_millis(10));
        (dir, script.to_string_lossy().to_string())
    }

    #[tokio::test]
    async fn init_success() {
        let (_dir, script) = mock_terraform("echo \"initialized\"\nexit 0\n");
        let work = TempDir::new().unwrap();
        let runner = TerraformRunner::new(&script, work.path().to_str().unwrap());
        let result = runner.init().await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("initialized"));
    }

    #[tokio::test]
    async fn init_failure() {
        let (_dir, script) = mock_terraform("echo \"init error\" >&2\nexit 1\n");
        let work = TempDir::new().unwrap();
        let runner = TerraformRunner::new(&script, work.path().to_str().unwrap());
        let result = runner.init().await.unwrap();
        assert!(!result.success);
        assert!(result.stderr.contains("init error"));
    }

    #[tokio::test]
    async fn apply_success() {
        let (_dir, script) = mock_terraform("echo \"Apply complete\"\nexit 0\n");
        let work = TempDir::new().unwrap();
        let runner = TerraformRunner::new(&script, work.path().to_str().unwrap());
        let result = runner.apply().await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("Apply complete"));
    }

    #[tokio::test]
    async fn destroy_success() {
        let (_dir, script) = mock_terraform("echo \"Destroy complete\"\nexit 0\n");
        let work = TempDir::new().unwrap();
        let runner = TerraformRunner::new(&script, work.path().to_str().unwrap());
        let result = runner.destroy().await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("Destroy complete"));
    }

    #[tokio::test]
    async fn plan_success() {
        let (_dir, script) = mock_terraform("echo \"Plan: 3\"\nexit 0\n");
        let work = TempDir::new().unwrap();
        let runner = TerraformRunner::new(&script, work.path().to_str().unwrap());
        let result = runner.plan().await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("Plan: 3"));
    }

    #[test]
    fn is_initialized_when_terraform_dir_exists() {
        let work = TempDir::new().unwrap();
        fs::create_dir(work.path().join(".terraform")).unwrap();
        let runner = TerraformRunner::new("terraform", work.path().to_str().unwrap());
        assert!(runner.is_initialized());
    }

    #[test]
    fn is_not_initialized_when_terraform_dir_missing() {
        let work = TempDir::new().unwrap();
        let runner = TerraformRunner::new("terraform", work.path().to_str().unwrap());
        assert!(!runner.is_initialized());
    }

    #[tokio::test]
    async fn verifies_correct_args_for_init() {
        let work = TempDir::new().unwrap();
        let args_file = work.path().join("args.txt");
        let body = format!(
            "echo \"$@\" > {}\necho ok\nexit 0\n",
            args_file.to_string_lossy()
        );
        let (_dir, script) = mock_terraform(&body);
        let runner = TerraformRunner::new(&script, work.path().to_str().unwrap());
        runner.init().await.unwrap();
        let recorded = fs::read_to_string(&args_file).unwrap();
        assert_eq!(recorded.trim(), "init -no-color");
    }

    #[tokio::test]
    async fn verifies_correct_args_for_apply() {
        let work = TempDir::new().unwrap();
        let args_file = work.path().join("args.txt");
        let body = format!(
            "echo \"$@\" > {}\necho ok\nexit 0\n",
            args_file.to_string_lossy()
        );
        let (_dir, script) = mock_terraform(&body);
        let runner = TerraformRunner::new(&script, work.path().to_str().unwrap());
        runner.apply().await.unwrap();
        let recorded = fs::read_to_string(&args_file).unwrap();
        assert_eq!(recorded.trim(), "apply -auto-approve -no-color");
    }

    #[tokio::test]
    async fn verifies_correct_args_for_destroy() {
        let work = TempDir::new().unwrap();
        let args_file = work.path().join("args.txt");
        let body = format!(
            "echo \"$@\" > {}\necho ok\nexit 0\n",
            args_file.to_string_lossy()
        );
        let (_dir, script) = mock_terraform(&body);
        let runner = TerraformRunner::new(&script, work.path().to_str().unwrap());
        runner.destroy().await.unwrap();
        let recorded = fs::read_to_string(&args_file).unwrap();
        assert_eq!(recorded.trim(), "destroy -auto-approve -no-color");
    }
}
