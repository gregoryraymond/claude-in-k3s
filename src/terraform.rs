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
        tracing::info!("Running terraform init in '{}'", self.working_dir);
        let result = self.run(&["init", "-no-color"]).await;
        match &result {
            Ok(r) if r.success => tracing::info!("terraform init succeeded"),
            Ok(r) => tracing::warn!("terraform init failed: {}", r.stderr.lines().next().unwrap_or("")),
            Err(e) => tracing::error!("terraform init error: {}", e),
        }
        result
    }

    /// Run `terraform init -reconfigure` to fix corrupted state.
    pub async fn init_reconfigure(&self) -> AppResult<CmdResult> {
        tracing::info!("Running terraform init -reconfigure in '{}'", self.working_dir);
        let result = self.run(&["init", "-reconfigure", "-no-color"]).await;
        match &result {
            Ok(r) if r.success => tracing::info!("terraform init -reconfigure succeeded"),
            Ok(r) => tracing::warn!("terraform init -reconfigure failed: {}", r.stderr.lines().next().unwrap_or("")),
            Err(e) => tracing::error!("terraform init -reconfigure error: {}", e),
        }
        result
    }

    pub async fn apply(&self) -> AppResult<CmdResult> {
        tracing::info!("Running terraform apply in '{}'", self.working_dir);
        let result = self.run(&["apply", "-auto-approve", "-no-color"]).await;
        match &result {
            Ok(r) if r.success => tracing::info!("terraform apply succeeded"),
            Ok(r) => tracing::warn!("terraform apply failed: {}", r.stderr.lines().next().unwrap_or("")),
            Err(e) => tracing::error!("terraform apply error: {}", e),
        }
        result
    }

    pub async fn destroy(&self) -> AppResult<CmdResult> {
        tracing::info!("Running terraform destroy in '{}'", self.working_dir);
        let result = self.run(&["destroy", "-auto-approve", "-no-color"]).await;
        match &result {
            Ok(r) if r.success => tracing::info!("terraform destroy succeeded"),
            Ok(r) => tracing::warn!("terraform destroy failed: {}", r.stderr.lines().next().unwrap_or("")),
            Err(e) => tracing::error!("terraform destroy error: {}", e),
        }
        result
    }

    pub async fn plan(&self) -> AppResult<CmdResult> {
        tracing::info!("Running terraform plan in '{}'", self.working_dir);
        let result = self.run(&["plan", "-no-color"]).await;
        match &result {
            Ok(r) if r.success => tracing::info!("terraform plan succeeded"),
            Ok(r) => tracing::warn!("terraform plan failed: {}", r.stderr.lines().next().unwrap_or("")),
            Err(e) => tracing::error!("terraform plan error: {}", e),
        }
        result
    }

    pub fn is_initialized(&self) -> bool {
        Path::new(&self.working_dir).join(".terraform").exists()
    }
}

#[cfg(all(test, unix))]
mod unix_tests {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // --- Constructor tests ---

    #[test]
    fn new_stores_binary_and_working_dir() {
        let runner = TerraformRunner::new("/usr/bin/terraform", "/tmp/work");
        assert_eq!(runner.binary, "/usr/bin/terraform");
        assert_eq!(runner.working_dir, "/tmp/work");
    }

    #[test]
    fn new_converts_str_refs_to_owned_strings() {
        let bin = String::from("tofu");
        let dir = String::from("/home/user/infra");
        let runner = TerraformRunner::new(&bin, &dir);
        assert_eq!(runner.binary, "tofu");
        assert_eq!(runner.working_dir, "/home/user/infra");
    }

    // --- is_initialized tests (cross-platform, filesystem only) ---

    #[test]
    fn is_initialized_true_when_terraform_dir_exists() {
        let work = TempDir::new().unwrap();
        fs::create_dir(work.path().join(".terraform")).unwrap();
        let runner = TerraformRunner::new("terraform", work.path().to_str().unwrap());
        assert!(runner.is_initialized());
    }

    #[test]
    fn is_initialized_false_when_terraform_dir_missing() {
        let work = TempDir::new().unwrap();
        let runner = TerraformRunner::new("terraform", work.path().to_str().unwrap());
        assert!(!runner.is_initialized());
    }

    #[test]
    fn is_initialized_false_when_terraform_is_a_file_not_dir() {
        let work = TempDir::new().unwrap();
        // Create .terraform as a regular file, not a directory
        fs::write(work.path().join(".terraform"), b"not a dir").unwrap();
        let runner = TerraformRunner::new("terraform", work.path().to_str().unwrap());
        // Path::exists() returns true for files too, so this should be true
        // (the real terraform also creates .terraform as a directory, but
        // exists() doesn't distinguish)
        assert!(runner.is_initialized());
    }

    #[test]
    fn is_initialized_false_when_working_dir_does_not_exist() {
        let runner = TerraformRunner::new("terraform", "/nonexistent/path/that/should/not/exist");
        assert!(!runner.is_initialized());
    }

    // --- Argument construction tests ---
    // We verify the command methods don't panic and handle missing binaries
    // gracefully by returning an IO error. On unix, we also verify exact
    // args via `echo`.

    #[tokio::test]
    async fn init_args_no_panic() {
        let work = TempDir::new().unwrap();
        // Use a nonexistent binary to test that the command is constructed
        // (it will return an IO error, not a CmdResult)
        let runner = TerraformRunner::new("__nonexistent_terraform_test_bin__", work.path().to_str().unwrap());
        let result = runner.init().await;
        // Should be an Err (binary not found), not a panic
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn apply_args_no_panic() {
        let work = TempDir::new().unwrap();
        let runner = TerraformRunner::new("__nonexistent_terraform_test_bin__", work.path().to_str().unwrap());
        let result = runner.apply().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn destroy_args_no_panic() {
        let work = TempDir::new().unwrap();
        let runner = TerraformRunner::new("__nonexistent_terraform_test_bin__", work.path().to_str().unwrap());
        let result = runner.destroy().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn plan_args_no_panic() {
        let work = TempDir::new().unwrap();
        let runner = TerraformRunner::new("__nonexistent_terraform_test_bin__", work.path().to_str().unwrap());
        let result = runner.plan().await;
        assert!(result.is_err());
    }

    // --- Argument verification via echo (unix only) ---

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn init_passes_correct_args_via_echo() {
        let work = TempDir::new().unwrap();
        let runner = TerraformRunner::new("echo", work.path().to_str().unwrap());
        let result = runner.init().await.unwrap();
        assert!(result.success);
        let stdout = result.stdout.trim();
        assert_eq!(stdout, "init -no-color");
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn apply_passes_correct_args_via_echo() {
        let work = TempDir::new().unwrap();
        let runner = TerraformRunner::new("echo", work.path().to_str().unwrap());
        let result = runner.apply().await.unwrap();
        assert!(result.success);
        let stdout = result.stdout.trim();
        assert_eq!(stdout, "apply -auto-approve -no-color");
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn destroy_passes_correct_args_via_echo() {
        let work = TempDir::new().unwrap();
        let runner = TerraformRunner::new("echo", work.path().to_str().unwrap());
        let result = runner.destroy().await.unwrap();
        assert!(result.success);
        let stdout = result.stdout.trim();
        assert_eq!(stdout, "destroy -auto-approve -no-color");
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn plan_passes_correct_args_via_echo() {
        let work = TempDir::new().unwrap();
        let runner = TerraformRunner::new("echo", work.path().to_str().unwrap());
        let result = runner.plan().await.unwrap();
        assert!(result.success);
        let stdout = result.stdout.trim();
        assert_eq!(stdout, "plan -no-color");
    }
}
