# Auto-Install Missing Dependencies Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** On app startup, detect missing tools (k3s, terraform, helm, docker) and show a setup screen that auto-installs them.

**Architecture:** New `src/deps.rs` module handles tool detection via `which` and version parsing. Installation uses binary downloads for terraform/helm and install scripts for k3s/docker. A new `SetupPanel` Slint component replaces the main TabWidget when deps are missing. A boolean property `all-deps-met` on `AppWindow` controls which view is shown.

**Tech Stack:** Rust std::process::Command for detection, tokio::process::Command for async installs, Slint UI

---

### Task 1: Add `detect_arch()` to platform.rs

**Files:**
- Modify: `src/platform.rs`

**Step 1: Write the failing test**

Add to `src/platform.rs` in the `tests` module:

```rust
#[test]
fn detect_arch_returns_known_value() {
    let arch = detect_arch();
    assert!(
        arch == "x86_64" || arch == "aarch64" || arch == "arm64",
        "unexpected arch: {}",
        arch
    );
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib platform::tests::detect_arch_returns_known_value`
Expected: FAIL — `detect_arch` not found

**Step 3: Write minimal implementation**

Add to `src/platform.rs` before the `tests` module:

```rust
pub fn detect_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64" // fallback
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --lib platform::tests::detect_arch_returns_known_value`
Expected: PASS

**Step 5: Commit**

```bash
git add src/platform.rs
git commit -m "feat: add detect_arch() to platform module"
```

---

### Task 2: Create `src/deps.rs` — tool detection

**Files:**
- Create: `src/deps.rs`
- Modify: `src/main.rs` (add `mod deps;`)

**Step 1: Write the failing test**

Create `src/deps.rs` with types, the public interface, and tests — but no implementation yet:

```rust
use crate::platform::Platform;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub enum ToolStatus {
    Found { version: String },
    Missing,
}

impl ToolStatus {
    pub fn is_found(&self) -> bool {
        matches!(self, ToolStatus::Found { .. })
    }
}

#[derive(Debug, Clone)]
pub struct DepsStatus {
    pub k3s: ToolStatus,
    pub terraform: ToolStatus,
    pub helm: ToolStatus,
    pub docker: ToolStatus,
}

impl DepsStatus {
    pub fn all_met(&self) -> bool {
        self.k3s.is_found()
            && self.terraform.is_found()
            && self.helm.is_found()
            && self.docker.is_found()
    }
}

/// Check if a single tool is available on PATH.
/// Runs `which <binary>` to detect presence, then `<binary> version` to get version string.
pub fn check_tool(binary: &str) -> ToolStatus {
    todo!()
}

/// Check all 4 required tools for the given platform.
pub fn check_all(platform: &Platform) -> DepsStatus {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_status_is_found() {
        let found = ToolStatus::Found { version: "1.0".into() };
        assert!(found.is_found());
        let missing = ToolStatus::Missing;
        assert!(!missing.is_found());
    }

    #[test]
    fn deps_status_all_met_true() {
        let status = DepsStatus {
            k3s: ToolStatus::Found { version: "v1.28".into() },
            terraform: ToolStatus::Found { version: "1.5.0".into() },
            helm: ToolStatus::Found { version: "3.12".into() },
            docker: ToolStatus::Found { version: "24.0".into() },
        };
        assert!(status.all_met());
    }

    #[test]
    fn deps_status_all_met_false_when_missing() {
        let status = DepsStatus {
            k3s: ToolStatus::Found { version: "v1.28".into() },
            terraform: ToolStatus::Missing,
            helm: ToolStatus::Found { version: "3.12".into() },
            docker: ToolStatus::Found { version: "24.0".into() },
        };
        assert!(!status.all_met());
    }

    #[test]
    fn check_tool_nonexistent_binary() {
        let status = check_tool("this_tool_does_not_exist_xyz_12345");
        assert_eq!(status, ToolStatus::Missing);
    }

    #[test]
    fn check_tool_existing_binary() {
        // `ls` exists on every Linux/macOS system
        let status = check_tool("ls");
        assert!(status.is_found());
    }

    #[test]
    fn check_all_returns_status_for_all_tools() {
        let status = check_all(&Platform::Linux);
        // We just verify the struct is populated — tools may or may not be installed
        let _ = status.k3s;
        let _ = status.terraform;
        let _ = status.helm;
        let _ = status.docker;
    }
}
```

Also add `mod deps;` to `src/main.rs` after the other module declarations.

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib deps::tests`
Expected: FAIL — `todo!()` panics

**Step 3: Implement check_tool and check_all**

Replace the `todo!()` bodies:

```rust
pub fn check_tool(binary: &str) -> ToolStatus {
    // Check if binary exists on PATH
    let which_result = Command::new("which")
        .arg(binary)
        .output();

    match which_result {
        Ok(output) if output.status.success() => {
            // Try to get version
            let version = Command::new(binary)
                .arg("version")
                .output()
                .ok()
                .and_then(|o| {
                    let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if out.is_empty() {
                        let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                        if err.is_empty() { None } else { Some(err) }
                    } else {
                        Some(out)
                    }
                })
                .unwrap_or_else(|| "unknown".to_string());

            // Take just the first line for display
            let version = version.lines().next().unwrap_or("unknown").to_string();
            ToolStatus::Found { version }
        }
        _ => ToolStatus::Missing,
    }
}

pub fn check_all(platform: &Platform) -> DepsStatus {
    use crate::platform;
    DepsStatus {
        k3s: check_tool("k3s"),
        terraform: check_tool(platform::terraform_binary(platform)),
        helm: check_tool(platform::helm_binary(platform)),
        docker: check_tool(platform::docker_binary(platform)),
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib deps::tests`
Expected: PASS (all 6 tests)

**Step 5: Run full test suite + clippy**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: All tests pass, zero warnings

**Step 6: Commit**

```bash
git add src/deps.rs src/main.rs
git commit -m "feat: add dependency detection module with check_tool and check_all"
```

---

### Task 3: Add install functions to `src/deps.rs`

**Files:**
- Modify: `src/deps.rs`

**Step 1: Write the failing test**

Add to `src/deps.rs`:

```rust
/// URL for downloading terraform binary
pub fn terraform_download_url(arch: &str) -> String {
    todo!()
}

/// URL for downloading helm binary
pub fn helm_download_url(arch: &str) -> String {
    todo!()
}

/// Install terraform by downloading the binary to ~/.local/bin/
pub async fn install_terraform() -> Result<String, String> {
    todo!()
}

/// Install helm by downloading the binary to ~/.local/bin/
pub async fn install_helm() -> Result<String, String> {
    todo!()
}

/// Install k3s using the official install script (requires sudo)
pub async fn install_k3s() -> Result<String, String> {
    todo!()
}

/// Install docker using the official install script (requires sudo)
pub async fn install_docker() -> Result<String, String> {
    todo!()
}
```

Add these tests:

```rust
#[test]
fn terraform_download_url_x86_64() {
    let url = terraform_download_url("x86_64");
    assert!(url.contains("terraform"));
    assert!(url.contains("linux_amd64"));
}

#[test]
fn terraform_download_url_aarch64() {
    let url = terraform_download_url("aarch64");
    assert!(url.contains("terraform"));
    assert!(url.contains("linux_arm64"));
}

#[test]
fn helm_download_url_x86_64() {
    let url = helm_download_url("x86_64");
    assert!(url.contains("helm"));
    assert!(url.contains("linux-amd64"));
}

#[test]
fn helm_download_url_aarch64() {
    let url = helm_download_url("aarch64");
    assert!(url.contains("helm"));
    assert!(url.contains("linux-arm64"));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib deps::tests`
Expected: FAIL — `todo!()` panics

**Step 3: Implement URL builders and install functions**

Add `use tokio::process::Command as AsyncCommand;` at the top of deps.rs.

Replace the `todo!()` bodies:

```rust
pub fn terraform_download_url(arch: &str) -> String {
    let arch_suffix = match arch {
        "aarch64" => "linux_arm64",
        _ => "linux_amd64",
    };
    format!(
        "https://releases.hashicorp.com/terraform/1.9.8/terraform_1.9.8_{}.zip",
        arch_suffix
    )
}

pub fn helm_download_url(arch: &str) -> String {
    let arch_suffix = match arch {
        "aarch64" => "linux-arm64",
        _ => "linux-amd64",
    };
    format!(
        "https://get.helm.sh/helm-v3.16.4-{}.tar.gz",
        arch_suffix
    )
}

pub async fn install_terraform() -> Result<String, String> {
    let arch = crate::platform::detect_arch();
    let url = terraform_download_url(arch);
    let install_dir = local_bin_dir()?;
    ensure_dir(&install_dir)?;

    let tmp_dir = std::env::temp_dir().join("claude-k3s-terraform-install");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("mkdir failed: {}", e))?;

    let zip_path = tmp_dir.join("terraform.zip");

    // Download
    run_async(&format!("curl -fsSL -o {} {}", zip_path.display(), url)).await?;

    // Extract
    run_async(&format!(
        "unzip -o {} -d {}",
        zip_path.display(),
        tmp_dir.display()
    ))
    .await?;

    // Move binary
    let src = tmp_dir.join("terraform");
    let dst = install_dir.join("terraform");
    std::fs::copy(&src, &dst).map_err(|e| format!("copy failed: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod failed: {}", e))?;
    }

    // Cleanup
    let _ = std::fs::remove_dir_all(&tmp_dir);

    Ok(format!("Terraform installed to {}", dst.display()))
}

pub async fn install_helm() -> Result<String, String> {
    let arch = crate::platform::detect_arch();
    let url = helm_download_url(arch);
    let install_dir = local_bin_dir()?;
    ensure_dir(&install_dir)?;

    let tmp_dir = std::env::temp_dir().join("claude-k3s-helm-install");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("mkdir failed: {}", e))?;

    let tar_path = tmp_dir.join("helm.tar.gz");

    // Download
    run_async(&format!("curl -fsSL -o {} {}", tar_path.display(), url)).await?;

    // Extract
    run_async(&format!(
        "tar xzf {} -C {}",
        tar_path.display(),
        tmp_dir.display()
    ))
    .await?;

    // Find extracted binary (inside linux-amd64/ or linux-arm64/ dir)
    let arch_dir = if arch == "aarch64" { "linux-arm64" } else { "linux-amd64" };
    let src = tmp_dir.join(arch_dir).join("helm");
    let dst = install_dir.join("helm");
    std::fs::copy(&src, &dst).map_err(|e| format!("copy failed: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("chmod failed: {}", e))?;
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);

    Ok(format!("Helm installed to {}", dst.display()))
}

pub async fn install_k3s() -> Result<String, String> {
    run_async("curl -sfL https://get.k3s.io | sudo sh -").await
}

pub async fn install_docker() -> Result<String, String> {
    run_async("curl -fsSL https://get.docker.com | sudo sh").await
}

fn local_bin_dir() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;
    Ok(home.join(".local").join("bin"))
}

fn ensure_dir(path: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("Failed to create {}: {}", path.display(), e))
}

async fn run_async(cmd: &str) -> Result<String, String> {
    let output = AsyncCommand::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .await
        .map_err(|e| format!("Failed to run '{}': {}", cmd, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        Err(format!("Command failed: {}\n{}", cmd, stderr))
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib deps::tests`
Expected: PASS (all 10 tests)

**Step 5: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: zero warnings

**Step 6: Commit**

```bash
git add src/deps.rs
git commit -m "feat: add dependency install functions with download URLs"
```

---

### Task 4: Add `DepsStatus` to `AppState`

**Files:**
- Modify: `src/app.rs`

**Step 1: Write the failing test**

Add a test to `src/app.rs` tests module:

```rust
#[test]
fn initial_state_has_deps_status() {
    let state = make_state();
    // Verify deps_status field exists and is accessible
    let _ = state.deps_status.all_met();
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib app::tests::initial_state_has_deps_status`
Expected: FAIL — no field `deps_status`

**Step 3: Add deps_status to AppState**

In `src/app.rs`:
1. Add `use crate::deps::{self, DepsStatus};` at the top
2. Add `pub deps_status: DepsStatus,` to the `AppState` struct
3. In `AppState::new()`, add `let deps_status = deps::check_all(&plat);` and include `deps_status` in the struct initialization
4. In the `make_state()` test helper, add:
   ```rust
   deps_status: DepsStatus {
       k3s: crate::deps::ToolStatus::Missing,
       terraform: crate::deps::ToolStatus::Missing,
       helm: crate::deps::ToolStatus::Missing,
       docker: crate::deps::ToolStatus::Missing,
   },
   ```

**Step 4: Run test to verify it passes**

Run: `cargo test --lib app::tests::initial_state_has_deps_status`
Expected: PASS

**Step 5: Run full test suite + clippy**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: All pass, zero warnings

**Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat: add DepsStatus to AppState with startup detection"
```

---

### Task 5: Create SetupPanel Slint component

**Files:**
- Create: `ui/components/setup-panel.slint`

**Step 1: Create the component**

Create `ui/components/setup-panel.slint`:

```slint
import { VerticalBox, HorizontalBox, Button, TextEdit, ScrollView } from "std-widgets.slint";

export component SetupPanel inherits VerticalBox {
    in property <bool> k3s-found: false;
    in property <bool> terraform-found: false;
    in property <bool> helm-found: false;
    in property <bool> docker-found: false;
    in property <string> k3s-version: "";
    in property <string> terraform-version: "";
    in property <string> helm-version: "";
    in property <string> docker-version: "";
    in property <string> install-log: "";
    in property <bool> is-installing: false;

    callback install-missing();
    callback continue-app();

    padding: 24px;
    spacing: 16px;

    Text {
        text: "Setup — Missing Dependencies";
        font-size: 22px;
        font-weight: 700;
        horizontal-alignment: center;
    }

    Text {
        text: "The following tools are required to run Claude in K3s.";
        font-size: 14px;
        color: #666;
        horizontal-alignment: center;
    }

    Rectangle {
        height: 1px;
        background: #ddd;
    }

    // Tool status list
    VerticalBox {
        spacing: 10px;
        horizontal-stretch: 0;

        for item in [
            { name: "k3s", found: k3s-found, version: k3s-version },
            { name: "terraform", found: terraform-found, version: terraform-version },
            { name: "helm", found: helm-found, version: helm-version },
            { name: "docker", found: docker-found, version: docker-version },
        ] : HorizontalBox {
            spacing: 12px;
            height: 32px;

            Rectangle {
                width: 20px;
                height: 20px;
                y: 6px;
                border-radius: 10px;
                background: item.found ? #4caf50 : #f44336;

                Text {
                    text: item.found ? "✓" : "✗";
                    color: white;
                    font-size: 13px;
                    font-weight: 700;
                    horizontal-alignment: center;
                    vertical-alignment: center;
                }
            }

            Text {
                text: item.name;
                font-size: 15px;
                font-weight: 600;
                vertical-alignment: center;
                min-width: 100px;
            }

            Text {
                text: item.found ? item.version : "Not installed";
                font-size: 13px;
                color: item.found ? #666 : #d32f2f;
                vertical-alignment: center;
            }
        }
    }

    Rectangle {
        height: 1px;
        background: #ddd;
    }

    // Action buttons
    HorizontalBox {
        spacing: 12px;
        alignment: center;

        Button {
            text: is-installing ? "Installing..." : "Install Missing";
            enabled: !is-installing && !(k3s-found && terraform-found && helm-found && docker-found);
            clicked => { install-missing(); }
        }

        Button {
            text: "Continue";
            enabled: !is-installing && k3s-found && terraform-found && helm-found && docker-found;
            clicked => { continue-app(); }
        }
    }

    // Install log
    TextEdit {
        text: install-log;
        read-only: true;
        font-size: 12px;
        vertical-stretch: 1;
        min-height: 150px;
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo build 2>&1 | head -5`
Expected: May warn about unused import, but should compile since the component isn't imported yet. We'll import it in Task 6.

**Step 3: Commit**

```bash
git add ui/components/setup-panel.slint
git commit -m "feat: create SetupPanel Slint component for dependency status"
```

---

### Task 6: Wire SetupPanel into AppWindow

**Files:**
- Modify: `ui/app-window.slint`
- Modify: `tests/ui_tests.rs`

**Step 1: Modify app-window.slint**

Add the import at the top of `ui/app-window.slint`:

```slint
import { SetupPanel } from "components/setup-panel.slint";
```

Add new properties to `AppWindow` (after the existing Settings state section):

```slint
    // Deps state
    in property <bool> all-deps-met: true;
    in property <bool> k3s-found: false;
    in property <bool> terraform-found: false;
    in property <bool> helm-found: false;
    in property <bool> docker-found: false;
    in property <string> k3s-version: "";
    in property <string> terraform-version: "";
    in property <string> helm-version: "";
    in property <string> docker-version: "";
    in property <string> install-log: "";
    in property <bool> is-installing: false;

    // Deps callbacks
    callback install-missing();
    callback continue-app();
```

Replace the `TabWidget` section (the `// Tab content` block) with a conditional:

```slint
        // Main content — conditional on deps
        if !all-deps-met : SetupPanel {
            vertical-stretch: 1;
            k3s-found: root.k3s-found;
            terraform-found: root.terraform-found;
            helm-found: root.helm-found;
            docker-found: root.docker-found;
            k3s-version: root.k3s-version;
            terraform-version: root.terraform-version;
            helm-version: root.helm-version;
            docker-version: root.docker-version;
            install-log: root.install-log;
            is-installing: root.is-installing;

            install-missing => { root.install-missing(); }
            continue-app => { root.continue-app(); }
        }

        if all-deps-met : TabWidget {
            vertical-stretch: 1;
            // ... (existing Tab content unchanged)
        }
```

Keep all the existing Tab contents inside the `if all-deps-met : TabWidget { ... }` block.

**Step 2: Add UI tests for new properties and callbacks**

Add to `tests/ui_tests.rs` (inside the existing test function, before the callback wiring section):

```rust
    // -- deps state defaults --
    assert!(ui.get_all_deps_met()); // default true so existing tests work
    assert!(!ui.get_k3s_found());
    assert!(!ui.get_terraform_found());
    assert!(!ui.get_helm_found());
    assert!(!ui.get_docker_found());
    assert_eq!(ui.get_k3s_version(), "");
    assert_eq!(ui.get_terraform_version(), "");
    assert_eq!(ui.get_helm_version(), "");
    assert_eq!(ui.get_docker_version(), "");
    assert_eq!(ui.get_install_log(), "");
    assert!(!ui.get_is_installing());

    // -- set/get deps state --
    ui.set_all_deps_met(false);
    assert!(!ui.get_all_deps_met());
    ui.set_k3s_found(true);
    assert!(ui.get_k3s_found());
    ui.set_k3s_version("v1.28.5+k3s1".into());
    assert_eq!(ui.get_k3s_version(), "v1.28.5+k3s1");
    ui.set_install_log("Installing terraform...".into());
    assert_eq!(ui.get_install_log(), "Installing terraform...");
    ui.set_is_installing(true);
    assert!(ui.get_is_installing());
    // Reset for remainder of tests
    ui.set_all_deps_met(true);
```

Add callback wiring tests alongside the existing ones:

```rust
    ui.on_install_missing(|| {});
    ui.on_continue_app(|| {});
```

And callback invocation tests:

```rust
    // install_missing
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_install_missing(move || { c.set(c.get() + 1); });
    ui.invoke_install_missing();
    assert_eq!(counter.get(), 1);

    // continue_app
    let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let c = counter.clone();
    ui.on_continue_app(move || { c.set(c.get() + 1); });
    ui.invoke_continue_app();
    assert_eq!(counter.get(), 1);
```

**Step 3: Run tests**

Run: `cargo test`
Expected: All pass (existing + new)

**Step 4: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: zero warnings

**Step 5: Commit**

```bash
git add ui/app-window.slint ui/components/setup-panel.slint tests/ui_tests.rs
git commit -m "feat: wire SetupPanel into AppWindow with conditional rendering"
```

---

### Task 7: Wire deps to main.rs — startup check, install, and continue

**Files:**
- Modify: `src/main.rs`

**Step 1: Add deps state initialization after `// Set initial UI state`**

After the existing initial state block (line ~49), add:

```rust
    // Set deps state
    {
        let s = state.lock().unwrap();
        let deps = &s.deps_status;
        let all_met = deps.all_met();
        ui.set_all_deps_met(all_met);

        let set_tool = |found: bool, version: &str, set_found: &dyn Fn(bool), set_ver: &dyn Fn(slint::SharedString)| {
            set_found(found);
            set_ver(version.into());
        };

        if let deps::ToolStatus::Found { ref version } = deps.k3s {
            ui.set_k3s_found(true);
            ui.set_k3s_version(version.clone().into());
        }
        if let deps::ToolStatus::Found { ref version } = deps.terraform {
            ui.set_terraform_found(true);
            ui.set_terraform_version(version.clone().into());
        }
        if let deps::ToolStatus::Found { ref version } = deps.helm {
            ui.set_helm_found(true);
            ui.set_helm_version(version.clone().into());
        }
        if let deps::ToolStatus::Found { ref version } = deps.docker {
            ui.set_docker_found(true);
            ui.set_docker_version(version.clone().into());
        }
    }
```

**Step 2: Add install-missing callback**

Add before the `// --- Periodic pod health check ---` section:

```rust
    // --- Install missing deps ---
    {
        let ui_handle = ui.as_weak();
        let state = state.clone();
        let rt_handle = rt.handle().clone();

        ui.on_install_missing(move || {
            let ui = ui_handle.clone();
            let state = state.clone();

            if let Some(u) = ui.upgrade() {
                u.set_is_installing(true);
                u.set_install_log("Starting installation of missing dependencies...\n".into());
            }

            rt_handle.spawn(async move {
                let deps = {
                    let s = state.lock().unwrap();
                    s.deps_status.clone()
                };

                let mut log = String::from("Starting installation of missing dependencies...\n");

                // Install each missing tool
                if !deps.terraform.is_found() {
                    log.push_str("\n--- Installing Terraform ---\n");
                    update_install_log(&ui, &log);
                    match deps::install_terraform().await {
                        Ok(msg) => log.push_str(&format!("✓ {}\n", msg)),
                        Err(e) => log.push_str(&format!("✗ Terraform install failed: {}\n", e)),
                    }
                    update_install_log(&ui, &log);
                }

                if !deps.helm.is_found() {
                    log.push_str("\n--- Installing Helm ---\n");
                    update_install_log(&ui, &log);
                    match deps::install_helm().await {
                        Ok(msg) => log.push_str(&format!("✓ {}\n", msg)),
                        Err(e) => log.push_str(&format!("✗ Helm install failed: {}\n", e)),
                    }
                    update_install_log(&ui, &log);
                }

                if !deps.k3s.is_found() {
                    log.push_str("\n--- Installing k3s ---\n");
                    log.push_str("(This requires sudo — enter your password if prompted)\n");
                    update_install_log(&ui, &log);
                    match deps::install_k3s().await {
                        Ok(msg) => log.push_str(&format!("✓ {}\n", msg)),
                        Err(e) => log.push_str(&format!("✗ k3s install failed: {}\n", e)),
                    }
                    update_install_log(&ui, &log);
                }

                if !deps.docker.is_found() {
                    log.push_str("\n--- Installing Docker ---\n");
                    log.push_str("(This requires sudo — enter your password if prompted)\n");
                    update_install_log(&ui, &log);
                    match deps::install_docker().await {
                        Ok(msg) => log.push_str(&format!("✓ {}\n", msg)),
                        Err(e) => log.push_str(&format!("✗ Docker install failed: {}\n", e)),
                    }
                    update_install_log(&ui, &log);
                }

                // Re-check all deps
                log.push_str("\n--- Re-checking dependencies ---\n");
                update_install_log(&ui, &log);
                let platform = {
                    let s = state.lock().unwrap();
                    s.platform.clone()
                };
                let new_status = deps::check_all(&platform);
                let all_met = new_status.all_met();

                {
                    let mut s = state.lock().unwrap();
                    s.deps_status = new_status.clone();
                }

                if all_met {
                    log.push_str("All dependencies satisfied!\n");
                } else {
                    log.push_str("Some dependencies are still missing. Check the log above.\n");
                }

                let final_log = log;
                let new_status_clone = new_status;
                slint::invoke_from_event_loop(move || {
                    if let Some(u) = ui.upgrade() {
                        u.set_install_log(final_log.into());
                        u.set_is_installing(false);
                        u.set_all_deps_met(all_met);

                        // Update individual statuses
                        if let deps::ToolStatus::Found { ref version } = new_status_clone.k3s {
                            u.set_k3s_found(true);
                            u.set_k3s_version(version.clone().into());
                        }
                        if let deps::ToolStatus::Found { ref version } = new_status_clone.terraform {
                            u.set_terraform_found(true);
                            u.set_terraform_version(version.clone().into());
                        }
                        if let deps::ToolStatus::Found { ref version } = new_status_clone.helm {
                            u.set_helm_found(true);
                            u.set_helm_version(version.clone().into());
                        }
                        if let deps::ToolStatus::Found { ref version } = new_status_clone.docker {
                            u.set_docker_found(true);
                            u.set_docker_version(version.clone().into());
                        }
                    }
                })
                .ok();
            });
        });
    }
```

Add the helper function at the bottom of the file (before the `tests` module):

```rust
fn update_install_log(ui: &slint::Weak<AppWindow>, log: &str) {
    let log = log.to_string();
    let ui = ui.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(u) = ui.upgrade() {
            u.set_install_log(log.into());
        }
    })
    .ok();
}
```

**Step 3: Add continue-app callback**

```rust
    // --- Continue from setup ---
    {
        let ui_handle = ui.as_weak();

        ui.on_continue_app(move || {
            if let Some(ui) = ui_handle.upgrade() {
                ui.set_all_deps_met(true);
            }
        });
    }
```

**Step 4: Add `use deps;` import**

Make sure `use deps;` or at least `deps::` references work. The `mod deps;` was added in Task 2. We reference `deps::ToolStatus`, `deps::check_all`, etc. via `crate::deps` or just `deps` since it's in the same crate root.

**Step 5: Run full test suite + clippy**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: All pass, zero warnings

**Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire dependency check and install to UI on startup"
```

---

### Task 8: Final validation

**Step 1: Run all tests**

Run: `cargo test`
Expected: All tests pass

**Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: zero warnings

**Step 3: Verify build**

Run: `cargo build`
Expected: clean build

**Step 4: Commit any remaining fixups**

If any fixups were needed, commit them.
