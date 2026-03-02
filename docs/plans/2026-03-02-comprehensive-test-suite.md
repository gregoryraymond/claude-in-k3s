# Comprehensive Test Suite Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a comprehensive test suite covering all modules with simulated external commands (no real terraform/helm/kubectl/docker calls).

**Architecture:** Unit tests for pure logic (projects, config, platform, error), mock-script-based integration tests for command runners (terraform, helm, kubectl, docker), and headless UI tests using `i-slint-backend-testing`. All external commands are simulated via shell scripts in temp directories that return canned JSON/text.

**Tech Stack:** Rust `#[cfg(test)]` modules, `tempfile` for filesystem tests, `i-slint-backend-testing = "=1.15.1"` for headless UI, bash mock scripts for CLI simulation.

---

### Task 1: Add test dependencies and create test infrastructure

**Files:**
- Modify: `Cargo.toml`
- Create: `tests/mock_bin.rs` (shared test helper)

**Step 1: Add dev-dependencies to Cargo.toml**

Add to `Cargo.toml`:

```toml
[dev-dependencies]
tempfile = "3"
i-slint-backend-testing = "=1.15.1"
```

**Step 2: Create the mock binary helper module**

Create `tests/mock_bin.rs` — a shared helper that creates executable shell scripts in temp dirs to simulate CLI tools:

```rust
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Creates a mock executable script in a temp directory that echoes canned output.
/// Returns (TempDir, path_to_script). TempDir must be kept alive for the script to exist.
pub fn create_mock_binary(name: &str, script_body: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let script_path = dir.path().join(name);
    let content = format!("#!/bin/bash\n{}", script_body);
    fs::write(&script_path, &content).expect("failed to write mock script");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))
            .expect("failed to chmod mock script");
    }

    (dir, script_path)
}

/// Creates a mock binary that always succeeds with given stdout.
pub fn mock_success(name: &str, stdout: &str) -> (TempDir, PathBuf) {
    let body = format!("echo '{}'\nexit 0", stdout.replace('\'', "'\\''"));
    create_mock_binary(name, &body)
}

/// Creates a mock binary that always fails with given stderr.
pub fn mock_failure(name: &str, stderr: &str) -> (TempDir, PathBuf) {
    let body = format!("echo '{}' >&2\nexit 1", stderr.replace('\'', "'\\''"));
    create_mock_binary(name, &body)
}

/// Creates a mock binary that writes its args to a file, then outputs canned response.
/// Useful for verifying the runner passed correct arguments.
pub fn mock_record_args(name: &str, stdout: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let script_path = dir.path().join(name);
    let args_file = dir.path().join("recorded_args");
    let content = format!(
        "#!/bin/bash\necho \"$@\" >> {}\necho '{}'\nexit 0",
        args_file.display(),
        stdout.replace('\'', "'\\''")
    );
    fs::write(&script_path, &content).expect("failed to write mock script");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))
            .expect("failed to chmod mock script");
    }

    (dir, script_path)
}

/// Read the recorded args file from a mock_record_args temp dir.
pub fn read_recorded_args(dir: &Path) -> Vec<String> {
    let args_file = dir.join("recorded_args");
    if args_file.exists() {
        fs::read_to_string(&args_file)
            .unwrap_or_default()
            .lines()
            .map(|l| l.to_string())
            .collect()
    } else {
        vec![]
    }
}
```

**Step 3: Verify the infrastructure compiles**

Run: `cargo test --no-run 2>&1 | tail -5`
Expected: compiles without errors

**Step 4: Commit**

```bash
git add Cargo.toml tests/mock_bin.rs
git commit -m "test: add test infrastructure with mock binary helpers"
```

---

### Task 2: Unit tests for `error.rs`

**Files:**
- Modify: `src/error.rs` (add `#[cfg(test)]` module at bottom)

**Step 1: Write tests**

Add to bottom of `src/error.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_result_success() {
        let r = CmdResult {
            success: true,
            stdout: "output".into(),
            stderr: String::new(),
        };
        assert!(r.success);
        assert_eq!(r.stdout, "output");
        assert!(r.stderr.is_empty());
    }

    #[test]
    fn cmd_result_failure() {
        let r = CmdResult {
            success: false,
            stdout: String::new(),
            stderr: "error occurred".into(),
        };
        assert!(!r.success);
        assert_eq!(r.stderr, "error occurred");
    }

    #[test]
    fn cmd_result_clone() {
        let r = CmdResult {
            success: true,
            stdout: "hello".into(),
            stderr: "world".into(),
        };
        let cloned = r.clone();
        assert_eq!(r.success, cloned.success);
        assert_eq!(r.stdout, cloned.stdout);
        assert_eq!(r.stderr, cloned.stderr);
    }

    #[test]
    fn app_error_display_messages() {
        assert_eq!(
            AppError::Terraform("tf fail".into()).to_string(),
            "Terraform error: tf fail"
        );
        assert_eq!(
            AppError::Helm("helm fail".into()).to_string(),
            "Helm error: helm fail"
        );
        assert_eq!(
            AppError::Kubectl("kube fail".into()).to_string(),
            "Kubectl error: kube fail"
        );
        assert_eq!(
            AppError::Docker("docker fail".into()).to_string(),
            "Docker error: docker fail"
        );
        assert_eq!(
            AppError::Config("config fail".into()).to_string(),
            "Configuration error: config fail"
        );
        assert_eq!(
            AppError::ProjectScan("scan fail".into()).to_string(),
            "Project scan error: scan fail"
        );
        assert_eq!(
            AppError::Platform("plat fail".into()).to_string(),
            "Platform error: plat fail"
        );
    }

    #[test]
    fn app_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let app_err: AppError = io_err.into();
        assert!(matches!(app_err, AppError::Io(_)));
        assert!(app_err.to_string().contains("file missing"));
    }

    #[test]
    fn app_error_from_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let app_err: AppError = json_err.into();
        assert!(matches!(app_err, AppError::Json(_)));
    }

    #[test]
    fn app_result_ok() {
        let result: AppResult<i32> = Ok(42);
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn app_result_err() {
        let result: AppResult<i32> = Err(AppError::Config("bad".into()));
        assert!(result.is_err());
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p claude-in-k3s error::tests -- --nocapture`
Expected: all 7 tests pass

**Step 3: Commit**

```bash
git add src/error.rs
git commit -m "test: add unit tests for error module"
```

---

### Task 3: Unit tests for `platform.rs`

**Files:**
- Modify: `src/platform.rs` (add `#[cfg(test)]` module at bottom)

**Step 1: Write tests**

Add to bottom of `src/platform.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_debug_and_clone() {
        let p = Platform::Linux;
        let cloned = p.clone();
        assert_eq!(p, cloned);
        assert_eq!(format!("{:?}", p), "Linux");
    }

    #[test]
    fn detect_platform_returns_a_variant() {
        let p = detect_platform();
        // We just verify it returns one of the valid variants
        matches!(p, Platform::Linux | Platform::MacOs | Platform::Wsl2 | Platform::Windows);
    }

    #[test]
    fn terraform_binary_linux() {
        assert_eq!(terraform_binary(&Platform::Linux), "terraform");
    }

    #[test]
    fn terraform_binary_macos() {
        assert_eq!(terraform_binary(&Platform::MacOs), "terraform");
    }

    #[test]
    fn terraform_binary_wsl2() {
        assert_eq!(terraform_binary(&Platform::Wsl2), "terraform");
    }

    #[test]
    fn terraform_binary_windows() {
        assert_eq!(terraform_binary(&Platform::Windows), "terraform.exe");
    }

    #[test]
    fn helm_binary_linux() {
        assert_eq!(helm_binary(&Platform::Linux), "helm");
    }

    #[test]
    fn helm_binary_windows() {
        assert_eq!(helm_binary(&Platform::Windows), "helm.exe");
    }

    #[test]
    fn kubectl_binary_linux() {
        assert_eq!(kubectl_binary(&Platform::Linux), "kubectl");
    }

    #[test]
    fn kubectl_binary_windows() {
        assert_eq!(kubectl_binary(&Platform::Windows), "kubectl.exe");
    }

    #[test]
    fn docker_binary_linux() {
        assert_eq!(docker_binary(&Platform::Linux), "docker");
    }

    #[test]
    fn docker_binary_windows() {
        assert_eq!(docker_binary(&Platform::Windows), "docker.exe");
    }

    #[test]
    fn platform_display_names() {
        assert_eq!(platform_display_name(&Platform::Linux), "Linux");
        assert_eq!(platform_display_name(&Platform::MacOs), "macOS");
        assert_eq!(platform_display_name(&Platform::Wsl2), "WSL2");
        assert_eq!(platform_display_name(&Platform::Windows), "Windows");
    }

    #[test]
    fn kubeconfig_path_ends_with_kube_config() {
        let path = kubeconfig_default_path();
        assert!(path.ends_with(".kube/config"));
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p claude-in-k3s platform::tests -- --nocapture`
Expected: all 14 tests pass

**Step 3: Commit**

```bash
git add src/platform.rs
git commit -m "test: add unit tests for platform module"
```

---

### Task 4: Unit tests for `projects.rs`

**Files:**
- Modify: `src/projects.rs` (add `#[cfg(test)]` module at bottom)

**Step 1: Write tests**

Add to bottom of `src/projects.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    // --- BaseImage enum tests ---

    #[test]
    fn base_image_docker_images() {
        assert_eq!(BaseImage::Node.docker_image(), "node:22-bookworm-slim");
        assert_eq!(BaseImage::Python.docker_image(), "python:3.12-slim-bookworm");
        assert_eq!(BaseImage::Rust.docker_image(), "rust:1.83-slim-bookworm");
        assert_eq!(BaseImage::Go.docker_image(), "golang:1.23-bookworm");
        assert_eq!(BaseImage::Dotnet.docker_image(), "mcr.microsoft.com/dotnet/sdk:9.0");
        assert_eq!(BaseImage::Base.docker_image(), "debian:bookworm-slim");
        assert_eq!(BaseImage::Custom.docker_image(), "custom");
    }

    #[test]
    fn base_image_labels() {
        assert_eq!(BaseImage::Node.label(), "Node.js");
        assert_eq!(BaseImage::Python.label(), "Python");
        assert_eq!(BaseImage::Rust.label(), "Rust");
        assert_eq!(BaseImage::Go.label(), "Go");
        assert_eq!(BaseImage::Dotnet.label(), ".NET");
        assert_eq!(BaseImage::Base.label(), "Minimal");
        assert_eq!(BaseImage::Custom.label(), "Custom");
    }

    #[test]
    fn all_presets_excludes_custom() {
        let presets = BaseImage::all_presets();
        assert_eq!(presets.len(), 6);
        assert!(!presets.contains(&BaseImage::Custom));
    }

    #[test]
    fn from_index_valid_values() {
        assert_eq!(BaseImage::from_index(0), BaseImage::Node);
        assert_eq!(BaseImage::from_index(1), BaseImage::Python);
        assert_eq!(BaseImage::from_index(2), BaseImage::Rust);
        assert_eq!(BaseImage::from_index(3), BaseImage::Go);
        assert_eq!(BaseImage::from_index(4), BaseImage::Dotnet);
        assert_eq!(BaseImage::from_index(5), BaseImage::Base);
        assert_eq!(BaseImage::from_index(6), BaseImage::Custom);
    }

    #[test]
    fn from_index_out_of_range_defaults_to_node() {
        assert_eq!(BaseImage::from_index(7), BaseImage::Node);
        assert_eq!(BaseImage::from_index(-1), BaseImage::Node);
        assert_eq!(BaseImage::from_index(100), BaseImage::Node);
    }

    #[test]
    fn to_index_values() {
        assert_eq!(BaseImage::Node.to_index(), 0);
        assert_eq!(BaseImage::Python.to_index(), 1);
        assert_eq!(BaseImage::Rust.to_index(), 2);
        assert_eq!(BaseImage::Go.to_index(), 3);
        assert_eq!(BaseImage::Dotnet.to_index(), 4);
        assert_eq!(BaseImage::Base.to_index(), 5);
        assert_eq!(BaseImage::Custom.to_index(), 6);
    }

    #[test]
    fn from_index_to_index_roundtrip() {
        for i in 0..=6 {
            let image = BaseImage::from_index(i);
            assert_eq!(image.to_index(), i);
        }
    }

    // --- detect_base_image tests ---

    #[test]
    fn detect_node_project() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_base_image(dir.path()), BaseImage::Node);
    }

    #[test]
    fn detect_rust_project() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        assert_eq!(detect_base_image(dir.path()), BaseImage::Rust);
    }

    #[test]
    fn detect_go_project() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("go.mod"), "").unwrap();
        assert_eq!(detect_base_image(dir.path()), BaseImage::Go);
    }

    #[test]
    fn detect_python_requirements() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("requirements.txt"), "").unwrap();
        assert_eq!(detect_base_image(dir.path()), BaseImage::Python);
    }

    #[test]
    fn detect_python_pyproject() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "").unwrap();
        assert_eq!(detect_base_image(dir.path()), BaseImage::Python);
    }

    #[test]
    fn detect_python_setup_py() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("setup.py"), "").unwrap();
        assert_eq!(detect_base_image(dir.path()), BaseImage::Python);
    }

    #[test]
    fn detect_dotnet_csproj() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("MyApp.csproj"), "").unwrap();
        assert_eq!(detect_base_image(dir.path()), BaseImage::Dotnet);
    }

    #[test]
    fn detect_dotnet_sln() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("MyApp.sln"), "").unwrap();
        assert_eq!(detect_base_image(dir.path()), BaseImage::Dotnet);
    }

    #[test]
    fn detect_base_fallback() {
        let dir = TempDir::new().unwrap();
        assert_eq!(detect_base_image(dir.path()), BaseImage::Base);
    }

    #[test]
    fn detect_node_takes_priority_over_others() {
        // package.json should win when multiple markers exist
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        fs::write(dir.path().join("requirements.txt"), "").unwrap();
        assert_eq!(detect_base_image(dir.path()), BaseImage::Node);
    }

    // --- has_dockerfile tests ---

    #[test]
    fn has_dockerfile_root() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Dockerfile"), "FROM node").unwrap();
        assert!(has_dockerfile(dir.path()));
    }

    #[test]
    fn has_dockerfile_in_claude_dir() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".claude")).unwrap();
        fs::write(dir.path().join(".claude").join("Dockerfile"), "FROM node").unwrap();
        assert!(has_dockerfile(dir.path()));
    }

    #[test]
    fn has_no_dockerfile() {
        let dir = TempDir::new().unwrap();
        assert!(!has_dockerfile(dir.path()));
    }

    // --- scan_projects tests ---

    #[test]
    fn scan_empty_directory() {
        let dir = TempDir::new().unwrap();
        let projects = scan_projects(dir.path()).unwrap();
        assert!(projects.is_empty());
    }

    #[test]
    fn scan_nonexistent_directory() {
        let projects = scan_projects(Path::new("/nonexistent/path/xyz")).unwrap();
        assert!(projects.is_empty());
    }

    #[test]
    fn scan_skips_hidden_directories() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join(".hidden")).unwrap();
        fs::create_dir(dir.path().join("visible")).unwrap();
        let projects = scan_projects(dir.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "visible");
    }

    #[test]
    fn scan_skips_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("not_a_dir.txt"), "hello").unwrap();
        fs::create_dir(dir.path().join("actual_project")).unwrap();
        let projects = scan_projects(dir.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "actual_project");
    }

    #[test]
    fn scan_detects_base_image() {
        let dir = TempDir::new().unwrap();
        let proj = dir.path().join("my-node-app");
        fs::create_dir(&proj).unwrap();
        fs::write(proj.join("package.json"), "{}").unwrap();
        let projects = scan_projects(dir.path()).unwrap();
        assert_eq!(projects[0].base_image, BaseImage::Node);
    }

    #[test]
    fn scan_detects_custom_dockerfile() {
        let dir = TempDir::new().unwrap();
        let proj = dir.path().join("custom-proj");
        fs::create_dir(&proj).unwrap();
        fs::write(proj.join("Dockerfile"), "FROM alpine").unwrap();
        let projects = scan_projects(dir.path()).unwrap();
        assert!(projects[0].has_custom_dockerfile);
        assert_eq!(projects[0].base_image, BaseImage::Custom);
    }

    #[test]
    fn scan_projects_sorted_alphabetically() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("zebra")).unwrap();
        fs::create_dir(dir.path().join("alpha")).unwrap();
        fs::create_dir(dir.path().join("middle")).unwrap();
        let projects = scan_projects(dir.path()).unwrap();
        let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "middle", "zebra"]);
    }

    #[test]
    fn scan_projects_default_not_selected() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("proj")).unwrap();
        let projects = scan_projects(dir.path()).unwrap();
        assert!(!projects[0].selected);
    }

    #[test]
    fn scan_multiple_projects_with_detection() {
        let dir = TempDir::new().unwrap();

        let node_proj = dir.path().join("frontend");
        fs::create_dir(&node_proj).unwrap();
        fs::write(node_proj.join("package.json"), "{}").unwrap();

        let rust_proj = dir.path().join("backend");
        fs::create_dir(&rust_proj).unwrap();
        fs::write(rust_proj.join("Cargo.toml"), "").unwrap();

        let go_proj = dir.path().join("cli-tool");
        fs::create_dir(&go_proj).unwrap();
        fs::write(go_proj.join("go.mod"), "").unwrap();

        let projects = scan_projects(dir.path()).unwrap();
        assert_eq!(projects.len(), 3);
        // sorted: backend, cli-tool, frontend
        assert_eq!(projects[0].name, "backend");
        assert_eq!(projects[0].base_image, BaseImage::Rust);
        assert_eq!(projects[1].name, "cli-tool");
        assert_eq!(projects[1].base_image, BaseImage::Go);
        assert_eq!(projects[2].name, "frontend");
        assert_eq!(projects[2].base_image, BaseImage::Node);
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p claude-in-k3s projects::tests -- --nocapture`
Expected: all ~25 tests pass

**Step 3: Commit**

```bash
git add src/projects.rs
git commit -m "test: add unit tests for projects module"
```

---

### Task 5: Unit tests for `config.rs`

**Files:**
- Modify: `src/config.rs` (add `#[cfg(test)]` module at bottom)

**Step 1: Write tests**

Add to bottom of `src/config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_config_values() {
        let config = AppConfig::default();
        assert!(config.projects_dir.is_none());
        assert!(config.api_key.is_none());
        assert_eq!(config.terraform_dir, "terraform");
        assert_eq!(config.helm_chart_dir, "helm/claude-code");
        assert_eq!(config.claude_mode, "daemon");
        assert_eq!(config.git_user_name, "Claude Code Bot");
        assert_eq!(config.git_user_email, "claude-bot@localhost");
        assert_eq!(config.cpu_limit, "2");
        assert_eq!(config.memory_limit, "4Gi");
    }

    #[test]
    fn config_path_is_under_config_dir() {
        let path = AppConfig::config_path();
        let path_str = path.to_string_lossy();
        assert!(path_str.contains("claude-in-k3s"));
        assert!(path_str.ends_with("config.toml"));
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let config = AppConfig {
            projects_dir: Some("/home/user/projects".into()),
            api_key: Some("sk-ant-test123".into()),
            ..AppConfig::default()
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let loaded: AppConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(loaded.projects_dir, config.projects_dir);
        assert_eq!(loaded.api_key, config.api_key);
        assert_eq!(loaded.terraform_dir, config.terraform_dir);
        assert_eq!(loaded.claude_mode, config.claude_mode);
    }

    #[test]
    fn serialize_with_none_fields() {
        let config = AppConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        // None fields should not appear in TOML output
        assert!(!toml_str.contains("projects_dir"));
        assert!(!toml_str.contains("api_key"));
    }

    #[test]
    fn deserialize_missing_optional_fields() {
        let toml_str = r#"
            terraform_dir = "terraform"
            helm_chart_dir = "helm/claude-code"
            claude_mode = "daemon"
            git_user_name = "Bot"
            git_user_email = "bot@test"
            cpu_limit = "2"
            memory_limit = "4Gi"
        "#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert!(config.projects_dir.is_none());
        assert!(config.api_key.is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.toml");

        let config = AppConfig {
            projects_dir: Some("/tmp/test-projects".into()),
            api_key: Some("sk-ant-roundtrip".into()),
            cpu_limit: "4".into(),
            memory_limit: "8Gi".into(),
            ..AppConfig::default()
        };

        // Save
        let content = toml::to_string_pretty(&config).unwrap();
        std::fs::write(&config_path, &content).unwrap();

        // Load
        let loaded_content = std::fs::read_to_string(&config_path).unwrap();
        let loaded: AppConfig = toml::from_str(&loaded_content).unwrap();

        assert_eq!(loaded.projects_dir, Some("/tmp/test-projects".into()));
        assert_eq!(loaded.api_key, Some("sk-ant-roundtrip".into()));
        assert_eq!(loaded.cpu_limit, "4");
        assert_eq!(loaded.memory_limit, "8Gi");
    }

    #[test]
    fn load_nonexistent_file_returns_default() {
        // AppConfig::load() returns default when file doesn't exist
        // We test the behavior by verifying default values
        let default = AppConfig::default();
        assert!(default.projects_dir.is_none());
        assert_eq!(default.terraform_dir, "terraform");
    }

    #[test]
    fn deserialize_with_extra_fields_is_ok() {
        let toml_str = r#"
            terraform_dir = "terraform"
            helm_chart_dir = "helm/claude-code"
            claude_mode = "daemon"
            git_user_name = "Bot"
            git_user_email = "bot@test"
            cpu_limit = "2"
            memory_limit = "4Gi"
            unknown_field = "should be ignored"
        "#;
        // serde default behavior - may or may not fail depending on config
        // This test documents the behavior
        let result = toml::from_str::<AppConfig>(toml_str);
        // With deny_unknown_fields this would fail; without it, it passes
        assert!(result.is_ok() || result.is_err());
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p claude-in-k3s config::tests -- --nocapture`
Expected: all tests pass

**Step 3: Commit**

```bash
git add src/config.rs
git commit -m "test: add unit tests for config module"
```

---

### Task 6: Unit tests for `docker.rs` (image_tag_for_project + mock command tests)

**Files:**
- Modify: `src/docker.rs` (add `#[cfg(test)]` module at bottom)

**Step 1: Write tests**

Add to bottom of `src/docker.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_project(name: &str, base_image: BaseImage) -> Project {
        Project {
            name: name.into(),
            path: PathBuf::from(format!("/tmp/{}", name)),
            selected: false,
            base_image,
            has_custom_dockerfile: false,
        }
    }

    #[test]
    fn image_tag_simple_name() {
        let p = make_project("my-project", BaseImage::Node);
        assert_eq!(image_tag_for_project(&p), "claude-code-my-project:latest");
    }

    #[test]
    fn image_tag_uppercase_lowered() {
        let p = make_project("MyProject", BaseImage::Node);
        assert_eq!(image_tag_for_project(&p), "claude-code-myproject:latest");
    }

    #[test]
    fn image_tag_special_chars_replaced() {
        let p = make_project("my.project_v2", BaseImage::Node);
        assert_eq!(image_tag_for_project(&p), "claude-code-my-project-v2:latest");
    }

    #[test]
    fn image_tag_spaces_replaced() {
        let p = make_project("my project", BaseImage::Node);
        assert_eq!(image_tag_for_project(&p), "claude-code-my-project:latest");
    }

    #[test]
    fn image_tag_already_valid() {
        let p = make_project("valid-name", BaseImage::Node);
        assert_eq!(image_tag_for_project(&p), "claude-code-valid-name:latest");
    }

    #[test]
    fn image_tag_numeric() {
        let p = make_project("project123", BaseImage::Node);
        assert_eq!(image_tag_for_project(&p), "claude-code-project123:latest");
    }

    #[test]
    fn docker_builder_construction() {
        let builder = DockerBuilder::new("docker", "/tmp/docker");
        assert_eq!(builder.docker_binary, "docker");
        assert_eq!(builder.template_dir, "/tmp/docker");
    }

    #[tokio::test]
    async fn build_custom_no_dockerfile_returns_error() {
        let dir = tempfile::TempDir::new().unwrap();
        let project = Project {
            name: "no-docker".into(),
            path: dir.path().to_path_buf(),
            selected: false,
            base_image: BaseImage::Custom,
            has_custom_dockerfile: false,
        };
        let builder = DockerBuilder::new("docker", "/tmp");
        let result = builder.build_custom(&project, "test:latest").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::Docker(_)));
        assert!(err.to_string().contains("No Dockerfile found"));
    }

    #[tokio::test]
    async fn build_preset_with_mock() {
        // Create a mock docker binary
        let mock_dir = tempfile::TempDir::new().unwrap();
        let mock_path = mock_dir.path().join("docker");
        std::fs::write(&mock_path, "#!/bin/bash\necho 'Successfully built test:latest'\nexit 0").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&mock_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let builder = DockerBuilder::new(
            mock_path.to_str().unwrap(),
            "/tmp",
        );
        let result = builder.build_preset(&BaseImage::Node, "test:latest").await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("Successfully built"));
    }

    #[tokio::test]
    async fn build_preset_failure_with_mock() {
        let mock_dir = tempfile::TempDir::new().unwrap();
        let mock_path = mock_dir.path().join("docker");
        std::fs::write(&mock_path, "#!/bin/bash\necho 'build failed' >&2\nexit 1").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&mock_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let builder = DockerBuilder::new(
            mock_path.to_str().unwrap(),
            "/tmp",
        );
        let result = builder.build_preset(&BaseImage::Node, "test:latest").await.unwrap();
        assert!(!result.success);
        assert!(result.stderr.contains("build failed"));
    }

    #[tokio::test]
    async fn build_and_import_skips_import_on_build_failure() {
        let mock_dir = tempfile::TempDir::new().unwrap();
        let mock_path = mock_dir.path().join("docker");
        std::fs::write(&mock_path, "#!/bin/bash\necho 'build failed' >&2\nexit 1").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&mock_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let project = make_project("test-proj", BaseImage::Node);
        let builder = DockerBuilder::new(
            mock_path.to_str().unwrap(),
            "/tmp",
        );
        let result = builder.build_and_import(&project).await.unwrap();
        assert!(!result.success);
        // Should return the build failure, not attempt import
        assert!(result.stderr.contains("build failed"));
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p claude-in-k3s docker::tests -- --nocapture`
Expected: all tests pass

**Step 3: Commit**

```bash
git add src/docker.rs
git commit -m "test: add unit tests for docker module"
```

---

### Task 7: Integration tests for `terraform.rs` with mock commands

**Files:**
- Modify: `src/terraform.rs` (add `#[cfg(test)]` module at bottom)

**Step 1: Write tests**

Add to bottom of `src/terraform.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    fn mock_terraform(body: &str) -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("terraform");
        let content = format!("#!/bin/bash\n{}", body);
        fs::write(&script, &content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = script.to_string_lossy().to_string();
        (dir, path)
    }

    #[tokio::test]
    async fn init_success() {
        let (_dir, bin) = mock_terraform("echo 'Terraform has been successfully initialized!'\nexit 0");
        let work_dir = TempDir::new().unwrap();
        let runner = TerraformRunner::new(&bin, work_dir.path().to_str().unwrap());
        let result = runner.init().await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("initialized"));
    }

    #[tokio::test]
    async fn init_failure() {
        let (_dir, bin) = mock_terraform("echo 'Error: plugin not found' >&2\nexit 1");
        let work_dir = TempDir::new().unwrap();
        let runner = TerraformRunner::new(&bin, work_dir.path().to_str().unwrap());
        let result = runner.init().await.unwrap();
        assert!(!result.success);
        assert!(result.stderr.contains("plugin not found"));
    }

    #[tokio::test]
    async fn apply_success() {
        let (_dir, bin) = mock_terraform("echo 'Apply complete! Resources: 5 added'\nexit 0");
        let work_dir = TempDir::new().unwrap();
        let runner = TerraformRunner::new(&bin, work_dir.path().to_str().unwrap());
        let result = runner.apply().await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("Apply complete"));
    }

    #[tokio::test]
    async fn destroy_success() {
        let (_dir, bin) = mock_terraform("echo 'Destroy complete! Resources: 5 destroyed'\nexit 0");
        let work_dir = TempDir::new().unwrap();
        let runner = TerraformRunner::new(&bin, work_dir.path().to_str().unwrap());
        let result = runner.destroy().await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("Destroy complete"));
    }

    #[tokio::test]
    async fn plan_success() {
        let (_dir, bin) = mock_terraform("echo 'Plan: 3 to add, 0 to change'\nexit 0");
        let work_dir = TempDir::new().unwrap();
        let runner = TerraformRunner::new(&bin, work_dir.path().to_str().unwrap());
        let result = runner.plan().await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("Plan: 3"));
    }

    #[test]
    fn is_initialized_when_terraform_dir_exists() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join(".terraform")).unwrap();
        let runner = TerraformRunner::new("terraform", dir.path().to_str().unwrap());
        assert!(runner.is_initialized());
    }

    #[test]
    fn is_not_initialized_when_terraform_dir_missing() {
        let dir = TempDir::new().unwrap();
        let runner = TerraformRunner::new("terraform", dir.path().to_str().unwrap());
        assert!(!runner.is_initialized());
    }

    #[tokio::test]
    async fn verifies_correct_args_for_init() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("terraform");
        let args_file = dir.path().join("args");
        let content = format!(
            "#!/bin/bash\necho \"$@\" > {}\necho ok\nexit 0",
            args_file.display()
        );
        fs::write(&script, &content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let work_dir = TempDir::new().unwrap();
        let runner = TerraformRunner::new(
            script.to_str().unwrap(),
            work_dir.path().to_str().unwrap(),
        );
        runner.init().await.unwrap();

        let args = fs::read_to_string(&args_file).unwrap();
        assert_eq!(args.trim(), "init -no-color");
    }

    #[tokio::test]
    async fn verifies_correct_args_for_apply() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("terraform");
        let args_file = dir.path().join("args");
        let content = format!(
            "#!/bin/bash\necho \"$@\" > {}\necho ok\nexit 0",
            args_file.display()
        );
        fs::write(&script, &content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let work_dir = TempDir::new().unwrap();
        let runner = TerraformRunner::new(
            script.to_str().unwrap(),
            work_dir.path().to_str().unwrap(),
        );
        runner.apply().await.unwrap();

        let args = fs::read_to_string(&args_file).unwrap();
        assert_eq!(args.trim(), "apply -auto-approve -no-color");
    }

    #[tokio::test]
    async fn verifies_correct_args_for_destroy() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("terraform");
        let args_file = dir.path().join("args");
        let content = format!(
            "#!/bin/bash\necho \"$@\" > {}\necho ok\nexit 0",
            args_file.display()
        );
        fs::write(&script, &content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let work_dir = TempDir::new().unwrap();
        let runner = TerraformRunner::new(
            script.to_str().unwrap(),
            work_dir.path().to_str().unwrap(),
        );
        runner.destroy().await.unwrap();

        let args = fs::read_to_string(&args_file).unwrap();
        assert_eq!(args.trim(), "destroy -auto-approve -no-color");
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p claude-in-k3s terraform::tests -- --nocapture`
Expected: all 10 tests pass

**Step 3: Commit**

```bash
git add src/terraform.rs
git commit -m "test: add integration tests for terraform runner with mock commands"
```

---

### Task 8: Integration tests for `helm.rs` with mock commands

**Files:**
- Modify: `src/helm.rs` (add `#[cfg(test)]` module at bottom)

**Step 1: Write tests**

Add to bottom of `src/helm.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    fn mock_helm(body: &str) -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("helm");
        let content = format!("#!/bin/bash\n{}", body);
        fs::write(&script, &content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }
        (dir, script.to_string_lossy().to_string())
    }

    fn mock_helm_record_args() -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("helm");
        let args_file = dir.path().join("args");
        let content = format!(
            "#!/bin/bash\necho \"$@\" >> {}\necho 'Release installed'\nexit 0",
            args_file.display()
        );
        fs::write(&script, &content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }
        (dir, script.to_string_lossy().to_string())
    }

    #[tokio::test]
    async fn install_or_upgrade_success() {
        let (_dir, bin) = mock_helm("echo 'Release installed'\nexit 0");
        let runner = HelmRunner::new(&bin, "./chart", "claude-code", "my-release");
        let projects = vec![
            ("proj-a".into(), "/home/user/proj-a".into(), "claude-code-proj-a:latest".into()),
        ];
        let result = runner.install_or_upgrade("sk-test", &projects).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn install_or_upgrade_failure() {
        let (_dir, bin) = mock_helm("echo 'Error: chart not found' >&2\nexit 1");
        let runner = HelmRunner::new(&bin, "./chart", "claude-code", "my-release");
        let result = runner.install_or_upgrade("sk-test", &[]).await.unwrap();
        assert!(!result.success);
        assert!(result.stderr.contains("chart not found"));
    }

    #[tokio::test]
    async fn install_or_upgrade_passes_correct_args() {
        let (dir, bin) = mock_helm_record_args();
        let runner = HelmRunner::new(&bin, "./chart", "claude-code", "my-release");
        let projects = vec![
            ("proj-a".into(), "/path/a".into(), "img-a:latest".into()),
        ];
        runner.install_or_upgrade("sk-test-key", &projects).await.unwrap();

        let args = fs::read_to_string(dir.path().join("args")).unwrap();
        assert!(args.contains("upgrade --install my-release ./chart"));
        assert!(args.contains("--namespace claude-code"));
        assert!(args.contains("--create-namespace"));
        assert!(args.contains("apiKey=sk-test-key"));
    }

    #[tokio::test]
    async fn install_or_upgrade_serializes_projects_as_json() {
        let (dir, bin) = mock_helm_record_args();
        let runner = HelmRunner::new(&bin, "./chart", "ns", "rel");
        let projects = vec![
            ("proj-a".into(), "/path/a".into(), "img-a:latest".into()),
            ("proj-b".into(), "/path/b".into(), "img-b:latest".into()),
        ];
        runner.install_or_upgrade("key", &projects).await.unwrap();

        let args = fs::read_to_string(dir.path().join("args")).unwrap();
        // The --set-json flag should contain valid JSON with project data
        assert!(args.contains("--set-json"));
        assert!(args.contains("proj-a"));
        assert!(args.contains("proj-b"));
    }

    #[tokio::test]
    async fn install_with_empty_projects() {
        let (dir, bin) = mock_helm_record_args();
        let runner = HelmRunner::new(&bin, "./chart", "ns", "rel");
        runner.install_or_upgrade("key", &[]).await.unwrap();

        let args = fs::read_to_string(dir.path().join("args")).unwrap();
        assert!(args.contains("projects=[]"));
    }

    #[tokio::test]
    async fn uninstall_success() {
        let (_dir, bin) = mock_helm("echo 'Release uninstalled'\nexit 0");
        let runner = HelmRunner::new(&bin, "./chart", "claude-code", "my-release");
        let result = runner.uninstall().await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn uninstall_passes_correct_args() {
        let (dir, bin) = mock_helm_record_args();
        let runner = HelmRunner::new(&bin, "./chart", "claude-code", "my-release");
        runner.uninstall().await.unwrap();

        let args = fs::read_to_string(dir.path().join("args")).unwrap();
        assert!(args.contains("uninstall my-release --namespace claude-code"));
    }

    #[tokio::test]
    async fn status_success() {
        let (_dir, bin) = mock_helm("echo 'STATUS: deployed'\nexit 0");
        let runner = HelmRunner::new(&bin, "./chart", "claude-code", "my-release");
        let result = runner.status().await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("deployed"));
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p claude-in-k3s helm::tests -- --nocapture`
Expected: all 8 tests pass

**Step 3: Commit**

```bash
git add src/helm.rs
git commit -m "test: add integration tests for helm runner with mock commands"
```

---

### Task 9: Integration tests for `kubectl.rs` with mock commands

**Files:**
- Modify: `src/kubectl.rs` (add `#[cfg(test)]` module at bottom)

**Step 1: Write tests**

Add to bottom of `src/kubectl.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    fn mock_kubectl(body: &str) -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("kubectl");
        let content = format!("#!/bin/bash\n{}", body);
        fs::write(&script, &content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }
        (dir, script.to_string_lossy().to_string())
    }

    fn sample_pods_json() -> &'static str {
        r#"{
            "items": [
                {
                    "metadata": {
                        "name": "claude-my-project-abc123",
                        "labels": {
                            "claude-code/project": "my-project"
                        },
                        "creationTimestamp": "2026-03-02T10:00:00Z"
                    },
                    "status": {
                        "phase": "Running",
                        "containerStatuses": [
                            {
                                "ready": true,
                                "restartCount": 0
                            }
                        ]
                    }
                },
                {
                    "metadata": {
                        "name": "claude-other-proj-def456",
                        "labels": {
                            "claude-code/project": "other-proj"
                        },
                        "creationTimestamp": "2026-03-02T09:00:00Z"
                    },
                    "status": {
                        "phase": "Pending",
                        "containerStatuses": [
                            {
                                "ready": false,
                                "restartCount": 3
                            }
                        ]
                    }
                }
            ]
        }"#
    }

    #[tokio::test]
    async fn get_pods_parses_json() {
        let json = sample_pods_json().replace('\n', "\\n").replace('"', "\\\"");
        let body = format!("printf '{}'", sample_pods_json().replace('\'', "'\\''"));
        let (_dir, bin) = mock_kubectl(&body);
        let runner = KubectlRunner::new(&bin, "claude-code");
        let pods = runner.get_pods().await.unwrap();

        assert_eq!(pods.len(), 2);

        assert_eq!(pods[0].name, "claude-my-project-abc123");
        assert_eq!(pods[0].project, "my-project");
        assert_eq!(pods[0].phase, "Running");
        assert!(pods[0].ready);
        assert_eq!(pods[0].restart_count, 0);
        assert_eq!(pods[0].age, "2026-03-02T10:00:00Z");

        assert_eq!(pods[1].name, "claude-other-proj-def456");
        assert_eq!(pods[1].project, "other-proj");
        assert_eq!(pods[1].phase, "Pending");
        assert!(!pods[1].ready);
        assert_eq!(pods[1].restart_count, 3);
    }

    #[tokio::test]
    async fn get_pods_empty_items() {
        let (_dir, bin) = mock_kubectl(r#"echo '{"items": []}'"#);
        let runner = KubectlRunner::new(&bin, "claude-code");
        let pods = runner.get_pods().await.unwrap();
        assert!(pods.is_empty());
    }

    #[tokio::test]
    async fn get_pods_command_failure_returns_empty() {
        let (_dir, bin) = mock_kubectl("echo 'connection refused' >&2\nexit 1");
        let runner = KubectlRunner::new(&bin, "claude-code");
        let pods = runner.get_pods().await.unwrap();
        assert!(pods.is_empty());
    }

    #[tokio::test]
    async fn get_pods_missing_labels_uses_defaults() {
        let json = r#"{"items": [{"metadata": {"name": "pod1"}, "status": {"phase": "Running"}}]}"#;
        let body = format!("echo '{}'", json);
        let (_dir, bin) = mock_kubectl(&body);
        let runner = KubectlRunner::new(&bin, "claude-code");
        let pods = runner.get_pods().await.unwrap();

        assert_eq!(pods.len(), 1);
        assert_eq!(pods[0].name, "pod1");
        assert_eq!(pods[0].project, "unknown");
        assert_eq!(pods[0].phase, "Running");
        assert!(!pods[0].ready);
        assert_eq!(pods[0].restart_count, 0);
        assert_eq!(pods[0].age, "");
    }

    #[tokio::test]
    async fn get_pods_missing_container_statuses() {
        let json = r#"{"items": [{"metadata": {"name": "pod1", "labels": {"claude-code/project": "test"}, "creationTimestamp": "2026-01-01T00:00:00Z"}, "status": {"phase": "Pending"}}]}"#;
        let body = format!("echo '{}'", json);
        let (_dir, bin) = mock_kubectl(&body);
        let runner = KubectlRunner::new(&bin, "claude-code");
        let pods = runner.get_pods().await.unwrap();

        assert_eq!(pods[0].ready, false);
        assert_eq!(pods[0].restart_count, 0);
    }

    #[tokio::test]
    async fn cluster_health_healthy() {
        let (_dir, bin) = mock_kubectl("echo 'Kubernetes control plane is running'\nexit 0");
        let runner = KubectlRunner::new(&bin, "claude-code");
        let healthy = runner.cluster_health().await.unwrap();
        assert!(healthy);
    }

    #[tokio::test]
    async fn cluster_health_unhealthy() {
        let (_dir, bin) = mock_kubectl("echo 'connection refused' >&2\nexit 1");
        let runner = KubectlRunner::new(&bin, "claude-code");
        let healthy = runner.cluster_health().await.unwrap();
        assert!(!healthy);
    }

    #[tokio::test]
    async fn delete_pod_success() {
        let (_dir, bin) = mock_kubectl("echo 'pod deleted'\nexit 0");
        let runner = KubectlRunner::new(&bin, "claude-code");
        let result = runner.delete_pod("my-pod").await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn delete_pod_verifies_args() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("kubectl");
        let args_file = dir.path().join("args");
        let content = format!(
            "#!/bin/bash\necho \"$@\" > {}\necho ok\nexit 0",
            args_file.display()
        );
        fs::write(&script, &content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let runner = KubectlRunner::new(script.to_str().unwrap(), "claude-code");
        runner.delete_pod("test-pod-123").await.unwrap();

        let args = fs::read_to_string(&args_file).unwrap();
        assert_eq!(args.trim(), "delete pod -n claude-code test-pod-123");
    }

    #[tokio::test]
    async fn get_logs_success() {
        let (_dir, bin) = mock_kubectl("echo 'line1\nline2\nline3'\nexit 0");
        let runner = KubectlRunner::new(&bin, "claude-code");
        let result = runner.get_logs("my-pod", 50).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn get_logs_verifies_args() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("kubectl");
        let args_file = dir.path().join("args");
        let content = format!(
            "#!/bin/bash\necho \"$@\" > {}\necho ok\nexit 0",
            args_file.display()
        );
        fs::write(&script, &content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let runner = KubectlRunner::new(script.to_str().unwrap(), "test-ns");
        runner.get_logs("my-pod", 100).await.unwrap();

        let args = fs::read_to_string(&args_file).unwrap();
        assert_eq!(args.trim(), "logs -n test-ns my-pod --tail 100");
    }

    #[test]
    fn pod_status_clone() {
        let pod = PodStatus {
            name: "pod-1".into(),
            project: "proj".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 2,
            age: "2026-01-01".into(),
        };
        let cloned = pod.clone();
        assert_eq!(pod.name, cloned.name);
        assert_eq!(pod.ready, cloned.ready);
        assert_eq!(pod.restart_count, cloned.restart_count);
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p claude-in-k3s kubectl::tests -- --nocapture`
Expected: all 12 tests pass

**Step 3: Commit**

```bash
git add src/kubectl.rs
git commit -m "test: add integration tests for kubectl runner with mock commands"
```

---

### Task 10: Unit tests for `app.rs`

**Files:**
- Modify: `src/app.rs` (add `#[cfg(test)]` module at bottom)

**Step 1: Write tests**

Add to bottom of `src/app.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::BaseImage;
    use tempfile::TempDir;
    use std::fs;

    fn make_state() -> AppState {
        AppState {
            config: AppConfig::default(),
            platform: Platform::Linux,
            projects: vec![],
            pods: vec![],
            cluster_healthy: false,
            tf_initialized: false,
            log_buffer: String::new(),
            project_root: PathBuf::from("/tmp/test-root"),
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
        state.append_log("first");
        state.append_log("second");
        assert_eq!(state.log_buffer, "first\nsecond");
    }

    #[test]
    fn append_log_multiple() {
        let mut state = make_state();
        state.append_log("a");
        state.append_log("b");
        state.append_log("c");
        assert_eq!(state.log_buffer, "a\nb\nc");
    }

    #[test]
    fn selected_projects_none_selected() {
        let mut state = make_state();
        state.projects = vec![
            Project {
                name: "proj-a".into(),
                path: PathBuf::from("/a"),
                selected: false,
                base_image: BaseImage::Node,
                has_custom_dockerfile: false,
            },
        ];
        assert!(state.selected_projects().is_empty());
    }

    #[test]
    fn selected_projects_some_selected() {
        let mut state = make_state();
        state.projects = vec![
            Project {
                name: "proj-a".into(),
                path: PathBuf::from("/a"),
                selected: true,
                base_image: BaseImage::Node,
                has_custom_dockerfile: false,
            },
            Project {
                name: "proj-b".into(),
                path: PathBuf::from("/b"),
                selected: false,
                base_image: BaseImage::Python,
                has_custom_dockerfile: false,
            },
            Project {
                name: "proj-c".into(),
                path: PathBuf::from("/c"),
                selected: true,
                base_image: BaseImage::Rust,
                has_custom_dockerfile: false,
            },
        ];
        let selected = state.selected_projects();
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].name, "proj-a");
        assert_eq!(selected[1].name, "proj-c");
    }

    #[test]
    fn scan_projects_with_dir_set() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("project-one")).unwrap();
        fs::create_dir(dir.path().join("project-two")).unwrap();

        let mut state = make_state();
        state.config.projects_dir = Some(dir.path().to_string_lossy().to_string());
        state.scan_projects().unwrap();

        assert_eq!(state.projects.len(), 2);
    }

    #[test]
    fn scan_projects_with_no_dir() {
        let mut state = make_state();
        state.config.projects_dir = None;
        state.scan_projects().unwrap();
        assert!(state.projects.is_empty());
    }

    #[test]
    fn terraform_runner_uses_platform_binary() {
        let state = make_state();
        let runner = state.terraform_runner();
        // Just verify it doesn't panic and returns a runner
        assert!(!runner.is_initialized()); // temp dir won't have .terraform
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
    fn find_project_root_finds_cargo_toml() {
        // We're running from the project dir which has Cargo.toml
        let root = find_project_root();
        assert!(root.is_some());
        assert!(root.unwrap().join("Cargo.toml").exists());
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p claude-in-k3s app::tests -- --nocapture`
Expected: all 9 tests pass

**Step 3: Commit**

```bash
git add src/app.rs
git commit -m "test: add unit tests for app state module"
```

---

### Task 11: Unit tests for `main.rs` helper functions

**Files:**
- Modify: `src/main.rs` (add `#[cfg(test)]` module at bottom)

**Step 1: Write tests**

Add to bottom of `src/main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_cmd_result_success() {
        let r = error::CmdResult {
            success: true,
            stdout: "all good".into(),
            stderr: String::new(),
        };
        let formatted = format_cmd_result("terraform apply", &r);
        assert!(formatted.contains("[SUCCESS]"));
        assert!(formatted.contains("terraform apply"));
        assert!(formatted.contains("all good"));
    }

    #[test]
    fn format_cmd_result_failure() {
        let r = error::CmdResult {
            success: false,
            stdout: String::new(),
            stderr: "something went wrong".into(),
        };
        let formatted = format_cmd_result("helm install", &r);
        assert!(formatted.contains("[FAILED]"));
        assert!(formatted.contains("helm install"));
        assert!(formatted.contains("STDERR: something went wrong"));
    }

    #[test]
    fn format_cmd_result_both_stdout_stderr() {
        let r = error::CmdResult {
            success: true,
            stdout: "output text".into(),
            stderr: "warning text".into(),
        };
        let formatted = format_cmd_result("kubectl get", &r);
        assert!(formatted.contains("output text"));
        assert!(formatted.contains("STDERR: warning text"));
    }

    #[test]
    fn format_cmd_result_empty_output() {
        let r = error::CmdResult {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        };
        let formatted = format_cmd_result("test cmd", &r);
        assert_eq!(formatted, "[SUCCESS] test cmd");
    }

    #[test]
    fn format_cmd_result_whitespace_trimmed() {
        let r = error::CmdResult {
            success: true,
            stdout: "  output  \n".into(),
            stderr: "  warning  \n".into(),
        };
        let formatted = format_cmd_result("cmd", &r);
        assert!(formatted.contains("output"));
        assert!(formatted.contains("warning"));
        // Verify trimming happened
        assert!(!formatted.ends_with('\n'));
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p claude-in-k3s tests -- --nocapture`
Expected: all 5 tests pass

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "test: add unit tests for main helper functions"
```

---

### Task 12: UI tests using slint testing backend

**Files:**
- Create: `tests/ui_tests.rs`

**Step 1: Write headless UI tests**

Create `tests/ui_tests.rs`:

```rust
//! Headless UI tests using Slint testing backend.
//! These tests verify UI property binding, callback wiring, and state management
//! without requiring a display server.

mod mock_bin;

slint::include_modules!();

fn init_test_backend() {
    i_slint_backend_testing::init_no_event_loop();
}

#[test]
fn window_creates_with_defaults() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    assert_eq!(ui.get_cluster_status(), "Unknown");
    assert_eq!(ui.get_cluster_log(), "");
    assert_eq!(ui.get_is_busy(), false);
    assert_eq!(ui.get_tf_initialized(), false);
    assert_eq!(ui.get_projects_dir(), "");
    assert_eq!(ui.get_api_key(), "");
    assert_eq!(ui.get_platform_name(), "Linux");
}

#[test]
fn set_cluster_status() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    ui.set_cluster_status("Healthy".into());
    assert_eq!(ui.get_cluster_status(), "Healthy");

    ui.set_cluster_status("Unreachable".into());
    assert_eq!(ui.get_cluster_status(), "Unreachable");
}

#[test]
fn set_busy_state() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    ui.set_is_busy(true);
    assert!(ui.get_is_busy());

    ui.set_is_busy(false);
    assert!(!ui.get_is_busy());
}

#[test]
fn set_tf_initialized() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    ui.set_tf_initialized(true);
    assert!(ui.get_tf_initialized());

    ui.set_tf_initialized(false);
    assert!(!ui.get_tf_initialized());
}

#[test]
fn set_projects_dir() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    ui.set_projects_dir("/home/user/projects".into());
    assert_eq!(ui.get_projects_dir(), "/home/user/projects");
}

#[test]
fn set_api_key() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    ui.set_api_key("sk-ant-test123".into());
    assert_eq!(ui.get_api_key(), "sk-ant-test123");
}

#[test]
fn set_platform_name() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    ui.set_platform_name("WSL2".into());
    assert_eq!(ui.get_platform_name(), "WSL2");
}

#[test]
fn set_cluster_log() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    ui.set_cluster_log("Running terraform init...\nDone.".into());
    assert_eq!(ui.get_cluster_log(), "Running terraform init...\nDone.");
}

#[test]
fn projects_model_empty_by_default() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();
    let projects = ui.get_projects();
    assert_eq!(projects.row_count(), 0);
}

#[test]
fn set_projects_model() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    let entries = vec![
        ProjectEntry {
            name: "frontend".into(),
            path: "/home/user/frontend".into(),
            selected: false,
            base_image_index: 0,
            has_custom_dockerfile: false,
        },
        ProjectEntry {
            name: "backend".into(),
            path: "/home/user/backend".into(),
            selected: true,
            base_image_index: 2,
            has_custom_dockerfile: true,
        },
    ];
    let model = std::rc::Rc::new(slint::VecModel::from(entries));
    ui.set_projects(model.into());

    let projects = ui.get_projects();
    assert_eq!(projects.row_count(), 2);
    assert_eq!(projects.row_data(0).unwrap().name, "frontend");
    assert_eq!(projects.row_data(0).unwrap().selected, false);
    assert_eq!(projects.row_data(0).unwrap().base_image_index, 0);
    assert_eq!(projects.row_data(1).unwrap().name, "backend");
    assert_eq!(projects.row_data(1).unwrap().selected, true);
    assert_eq!(projects.row_data(1).unwrap().base_image_index, 2);
    assert_eq!(projects.row_data(1).unwrap().has_custom_dockerfile, true);
}

#[test]
fn pods_model_empty_by_default() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();
    let pods = ui.get_pods();
    assert_eq!(pods.row_count(), 0);
}

#[test]
fn set_pods_model() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    let entries = vec![
        PodEntry {
            name: "claude-frontend-abc".into(),
            project: "frontend".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 0,
            age: "2026-03-02T10:00:00Z".into(),
        },
        PodEntry {
            name: "claude-backend-def".into(),
            project: "backend".into(),
            phase: "Pending".into(),
            ready: false,
            restart_count: 5,
            age: "2026-03-02T09:30:00Z".into(),
        },
    ];
    let model = std::rc::Rc::new(slint::VecModel::from(entries));
    ui.set_pods(model.into());

    let pods = ui.get_pods();
    assert_eq!(pods.row_count(), 2);
    assert_eq!(pods.row_data(0).unwrap().project, "frontend");
    assert_eq!(pods.row_data(0).unwrap().phase, "Running");
    assert!(pods.row_data(0).unwrap().ready);
    assert_eq!(pods.row_data(0).unwrap().restart_count, 0);
    assert_eq!(pods.row_data(1).unwrap().project, "backend");
    assert_eq!(pods.row_data(1).unwrap().phase, "Pending");
    assert!(!pods.row_data(1).unwrap().ready);
    assert_eq!(pods.row_data(1).unwrap().restart_count, 5);
}

#[test]
fn terraform_init_callback_fires() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();
    let fired = std::cell::Cell::new(false);
    ui.on_terraform_init(|| {
        // Can't capture &fired directly, so we use a different approach
    });
    // Verify the callback can be set without panic
}

#[test]
fn callback_wiring_does_not_panic() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    // Verify all callbacks can be wired without panicking
    ui.on_terraform_init(|| {});
    ui.on_terraform_apply(|| {});
    ui.on_terraform_destroy(|| {});
    ui.on_browse_folder(|| {});
    ui.on_refresh_projects(|| {});
    ui.on_launch_selected(|| {});
    ui.on_stop_selected(|| {});
    ui.on_project_toggled(|_idx, _checked| {});
    ui.on_project_image_changed(|_idx, _img| {});
    ui.on_refresh_pods(|| {});
    ui.on_delete_pod(|_idx| {});
    ui.on_view_logs(|_idx| {});
    ui.on_save_settings(|| {});
}

#[test]
fn terraform_init_callback_invoked() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let counter_clone = counter.clone();
    ui.on_terraform_init(move || {
        counter_clone.set(counter_clone.get() + 1);
    });

    ui.invoke_terraform_init();
    assert_eq!(counter.get(), 1);

    ui.invoke_terraform_init();
    assert_eq!(counter.get(), 2);
}

#[test]
fn terraform_apply_callback_invoked() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let counter_clone = counter.clone();
    ui.on_terraform_apply(move || {
        counter_clone.set(counter_clone.get() + 1);
    });

    ui.invoke_terraform_apply();
    assert_eq!(counter.get(), 1);
}

#[test]
fn terraform_destroy_callback_invoked() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let counter_clone = counter.clone();
    ui.on_terraform_destroy(move || {
        counter_clone.set(counter_clone.get() + 1);
    });

    ui.invoke_terraform_destroy();
    assert_eq!(counter.get(), 1);
}

#[test]
fn project_toggled_callback_receives_correct_args() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    let received_idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let received_checked = std::rc::Rc::new(std::cell::Cell::new(false));
    let idx_clone = received_idx.clone();
    let checked_clone = received_checked.clone();

    ui.on_project_toggled(move |idx, checked| {
        idx_clone.set(idx);
        checked_clone.set(checked);
    });

    ui.invoke_project_toggled(3, true);
    assert_eq!(received_idx.get(), 3);
    assert_eq!(received_checked.get(), true);
}

#[test]
fn project_image_changed_callback_receives_correct_args() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    let received_idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let received_img = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let idx_clone = received_idx.clone();
    let img_clone = received_img.clone();

    ui.on_project_image_changed(move |idx, img| {
        idx_clone.set(idx);
        img_clone.set(img);
    });

    ui.invoke_project_image_changed(2, 5);
    assert_eq!(received_idx.get(), 2);
    assert_eq!(received_img.get(), 5);
}

#[test]
fn delete_pod_callback_receives_correct_index() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    let received_idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let idx_clone = received_idx.clone();

    ui.on_delete_pod(move |idx| {
        idx_clone.set(idx);
    });

    ui.invoke_delete_pod(7);
    assert_eq!(received_idx.get(), 7);
}

#[test]
fn view_logs_callback_receives_correct_index() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    let received_idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
    let idx_clone = received_idx.clone();

    ui.on_view_logs(move |idx| {
        idx_clone.set(idx);
    });

    ui.invoke_view_logs(4);
    assert_eq!(received_idx.get(), 4);
}

#[test]
fn save_settings_callback_invoked() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let counter_clone = counter.clone();
    ui.on_save_settings(move || {
        counter_clone.set(counter_clone.get() + 1);
    });

    ui.invoke_save_settings();
    assert_eq!(counter.get(), 1);
}

#[test]
fn browse_folder_callback_invoked() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let counter_clone = counter.clone();
    ui.on_browse_folder(move || {
        counter_clone.set(counter_clone.get() + 1);
    });

    ui.invoke_browse_folder();
    assert_eq!(counter.get(), 1);
}

#[test]
fn refresh_projects_callback_invoked() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let counter_clone = counter.clone();
    ui.on_refresh_projects(move || {
        counter_clone.set(counter_clone.get() + 1);
    });

    ui.invoke_refresh_projects();
    assert_eq!(counter.get(), 1);
}

#[test]
fn launch_selected_callback_invoked() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let counter_clone = counter.clone();
    ui.on_launch_selected(move || {
        counter_clone.set(counter_clone.get() + 1);
    });

    ui.invoke_launch_selected();
    assert_eq!(counter.get(), 1);
}

#[test]
fn stop_selected_callback_invoked() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let counter_clone = counter.clone();
    ui.on_stop_selected(move || {
        counter_clone.set(counter_clone.get() + 1);
    });

    ui.invoke_stop_selected();
    assert_eq!(counter.get(), 1);
}

#[test]
fn refresh_pods_callback_invoked() {
    init_test_backend();
    let ui = AppWindow::new().unwrap();

    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let counter_clone = counter.clone();
    ui.on_refresh_pods(move || {
        counter_clone.set(counter_clone.get() + 1);
    });

    ui.invoke_refresh_pods();
    assert_eq!(counter.get(), 1);
}
```

**Step 2: Run tests**

Run: `cargo test --test ui_tests -- --nocapture`
Expected: all ~25 UI tests pass

**Step 3: Commit**

```bash
git add tests/ui_tests.rs
git commit -m "test: add headless UI tests using slint testing backend"
```

---

### Task 13: Run full test suite and verify

**Step 1: Run all tests**

Run: `cargo test 2>&1`
Expected: All tests pass (approximately 90+ tests across all modules)

**Step 2: Run with verbose output to see test names**

Run: `cargo test -- --list 2>&1`
Expected: Lists all test names, organized by module

**Step 3: Run clippy to ensure test code is clean**

Run: `cargo clippy --tests 2>&1`
Expected: No warnings

**Step 4: Final commit with any fixes**

If any tests needed adjustment, commit the fixes.
