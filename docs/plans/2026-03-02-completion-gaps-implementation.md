# Completion Gaps Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Complete all 7 remaining gaps to bring the project to full completion.

**Architecture:** Each task is independent and can be committed separately. Order is from simplest to most complex: cleanup first, then feature additions, then documentation last.

**Tech Stack:** Rust, Slint UI, chrono, Terraform HCL, Markdown

---

### Task 1: Clean Up Terraform Providers

**Files:**
- Modify: `terraform/providers.tf`

**Step 1: Remove unused provider blocks**

Replace the entire file content with:

```hcl
terraform {
  required_version = ">= 1.5.0"
}
```

This removes the unused `hashicorp/null` and `hashicorp/local` provider declarations. Only `terraform_data` (a built-in resource since Terraform 1.4) is used in the `.tf` files, so no external providers are needed.

**Step 2: Validate terraform config**

Run: `terraform -chdir=terraform validate` (if terraform is initialized) or `terraform -chdir=terraform fmt -check`
Expected: Clean output, no errors.

**Step 3: Commit**

```bash
git add terraform/providers.tf
git commit -m "chore: remove unused null and local terraform provider declarations"
```

---

### Task 2: Clean Up Unused Test Helpers

**Files:**
- Delete: `tests/mock_bin.rs`

**Step 1: Verify no references to mock_bin**

Search for any `mod mock_bin` or `use mock_bin` in the codebase. There should be none — each test module creates its own inline mock scripts.

Run: `grep -r "mock_bin" tests/ src/`
Expected: Only hits in `tests/mock_bin.rs` itself (the file we're deleting).

**Step 2: Delete the file**

```bash
rm tests/mock_bin.rs
```

**Step 3: Run tests to confirm nothing breaks**

Run: `cargo test`
Expected: All 114 tests pass.

**Step 4: Commit**

```bash
git add -A tests/mock_bin.rs
git commit -m "chore: remove unused mock_bin test helpers"
```

---

### Task 3: Human-Friendly Pod Age

**Files:**
- Modify: `src/kubectl.rs` (add `format_age()`, call it in `get_pods()`)
- Modify: `src/kubectl.rs` (update tests)

**Step 1: Write tests for format_age**

Add these tests to the `#[cfg(test)] mod tests` block in `src/kubectl.rs`:

```rust
#[test]
fn format_age_days_and_hours() {
    // 2 days and 3 hours ago
    let now = chrono::Utc::now();
    let ts = now - chrono::Duration::hours(51); // 2d 3h
    let formatted = format_age(&ts.to_rfc3339());
    assert!(formatted.contains("2d"), "expected '2d' in '{}'", formatted);
    assert!(formatted.contains("3h"), "expected '3h' in '{}'", formatted);
}

#[test]
fn format_age_hours_and_minutes() {
    let now = chrono::Utc::now();
    let ts = now - chrono::Duration::minutes(135); // 2h 15m
    let formatted = format_age(&ts.to_rfc3339());
    assert!(formatted.contains("2h"), "expected '2h' in '{}'", formatted);
    assert!(formatted.contains("15m"), "expected '15m' in '{}'", formatted);
}

#[test]
fn format_age_minutes_only() {
    let now = chrono::Utc::now();
    let ts = now - chrono::Duration::minutes(45);
    let formatted = format_age(&ts.to_rfc3339());
    assert_eq!(formatted, "45m");
}

#[test]
fn format_age_less_than_one_minute() {
    let now = chrono::Utc::now();
    let ts = now - chrono::Duration::seconds(30);
    let formatted = format_age(&ts.to_rfc3339());
    assert_eq!(formatted, "< 1m");
}

#[test]
fn format_age_invalid_timestamp() {
    let formatted = format_age("not-a-timestamp");
    assert_eq!(formatted, "unknown");
}

#[test]
fn format_age_empty_string() {
    let formatted = format_age("");
    assert_eq!(formatted, "unknown");
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib kubectl::tests::format_age`
Expected: FAIL — `format_age` function not found.

**Step 3: Implement format_age**

Add this function to `src/kubectl.rs` (above the `impl KubectlRunner` block, after the imports):

```rust
use chrono::{DateTime, Utc};

fn format_age(timestamp: &str) -> String {
    let parsed = timestamp
        .parse::<DateTime<Utc>>()
        .or_else(|_| DateTime::parse_from_rfc3339(timestamp).map(|dt| dt.with_timezone(&Utc)));

    match parsed {
        Ok(created) => {
            let duration = Utc::now() - created;
            let total_minutes = duration.num_minutes();

            if total_minutes < 1 {
                "< 1m".to_string()
            } else if total_minutes < 60 {
                format!("{}m", total_minutes)
            } else {
                let hours = duration.num_hours();
                let days = hours / 24;
                let remaining_hours = hours % 24;
                let remaining_minutes = total_minutes % 60;

                if days > 0 {
                    format!("{}d {}h", days, remaining_hours)
                } else {
                    format!("{}h {}m", hours, remaining_minutes)
                }
            }
        }
        Err(_) => "unknown".to_string(),
    }
}
```

**Step 4: Update get_pods() to use format_age**

In the `get_pods()` method, change the age assignment from:

```rust
let age = item["metadata"]["creationTimestamp"]
    .as_str()
    .unwrap_or("")
    .to_string();
```

to:

```rust
let age = format_age(
    item["metadata"]["creationTimestamp"]
        .as_str()
        .unwrap_or(""),
);
```

**Step 5: Update existing tests that check `.age`**

In `get_pods_parses_json` test, change:
```rust
assert_eq!(pods[0].age, "2025-01-15T10:30:00Z");
```
to:
```rust
// Age is now a formatted relative string, not raw timestamp
assert!(!pods[0].age.is_empty());
assert_ne!(pods[0].age, "2025-01-15T10:30:00Z"); // Should be formatted
```
And similarly for `pods[1].age`.

In `get_pods_missing_labels_uses_defaults`:
```rust
assert_eq!(pods[0].age, "");
```
changes to:
```rust
assert_eq!(pods[0].age, "unknown");
```

In `get_pods_missing_container_statuses`:
```rust
assert_eq!(pods[0].age, "2025-02-01T12:00:00Z");
```
changes to:
```rust
assert!(!pods[0].age.is_empty());
assert_ne!(pods[0].age, "2025-02-01T12:00:00Z");
```

In `pod_status_clone`:
```rust
age: "2025-01-01T00:00:00Z".to_string(),
```
This is fine as-is (it's just testing Clone, the value doesn't matter).

**Step 6: Run tests**

Run: `cargo test`
Expected: All tests pass, including the new `format_age_*` tests.

**Step 7: Commit**

```bash
git add src/kubectl.rs
git commit -m "feat: display human-friendly pod age using chrono"
```

---

### Task 4: Wire terraform plan() and helm status()

**Files:**
- Modify: `ui/components/cluster-panel.slint` (add Plan and Helm Status buttons)
- Modify: `ui/app-window.slint` (add callbacks)
- Modify: `src/main.rs` (wire callbacks)
- Modify: `src/terraform.rs` (remove `#[allow(dead_code)]` from `plan()`)
- Modify: `src/helm.rs` (remove `#[allow(dead_code)]` from `status()`)
- Modify: `tests/ui_tests.rs` (add callback tests)

**Step 1: Add callbacks to app-window.slint**

In `ui/app-window.slint`, in the "Cluster callbacks" section (after `callback terraform-destroy();`), add:

```slint
callback terraform-plan();
callback helm-status();
```

**Step 2: Add buttons to cluster-panel.slint**

In `ui/components/cluster-panel.slint`, add `callback terraform-plan();` and `callback helm-status();` to the component declaration.

In the Terraform Lifecycle HorizontalBox (after the Destroy button), add:

```slint
Button {
    text: "Plan";
    enabled: !is-busy && tf-initialized;
    clicked => { terraform-plan(); }
}
```

Add a new GroupBox after the Terraform Lifecycle GroupBox:

```slint
GroupBox {
    title: "Helm Release";

    HorizontalBox {
        spacing: 8px;
        alignment: start;

        Button {
            text: "Status";
            enabled: !is-busy;
            clicked => { helm-status(); }
        }
    }
}
```

Wire callbacks in the Tab section of `app-window.slint` where ClusterPanel is used:

```slint
terraform-plan => { root.terraform-plan(); }
helm-status => { root.helm-status(); }
```

**Step 3: Wire callbacks in main.rs**

Add terraform_plan callback (after the terraform_destroy block):

```rust
{
    let ui_handle = ui.as_weak();
    let state = state.clone();
    let rt_handle = rt.handle().clone();

    ui.on_terraform_plan(move || {
        let ui = ui_handle.clone();
        let state = state.clone();
        set_busy(&ui, true);
        append_log(&state, "Running terraform plan...");
        sync_log(&ui, &state);

        rt_handle.spawn(async move {
            let runner = {
                let s = state.lock().unwrap();
                s.terraform_runner()
            };
            let result = runner.plan().await;

            slint::invoke_from_event_loop(move || {
                match result {
                    Ok(r) => {
                        append_log(&state, &format_cmd_result("terraform plan", &r));
                    }
                    Err(e) => append_log(&state, &format!("Error: {}", e)),
                }
                set_busy(&ui, false);
                sync_log(&ui, &state);
            })
            .ok();
        });
    });
}
```

Add helm_status callback (after the stop_selected block):

```rust
{
    let ui_handle = ui.as_weak();
    let state = state.clone();
    let rt_handle = rt.handle().clone();

    ui.on_helm_status(move || {
        let ui = ui_handle.clone();
        let state = state.clone();
        set_busy(&ui, true);
        append_log(&state, "Checking Helm release status...");
        sync_log(&ui, &state);

        let helm_runner = {
            let s = state.lock().unwrap();
            s.helm_runner()
        };

        rt_handle.spawn(async move {
            let result = helm_runner.status().await;

            slint::invoke_from_event_loop(move || {
                match result {
                    Ok(r) => {
                        append_log(&state, &format_cmd_result("helm status", &r));
                    }
                    Err(e) => append_log(&state, &format!("Helm status error: {}", e)),
                }
                set_busy(&ui, false);
                sync_log(&ui, &state);
            })
            .ok();
        });
    });
}
```

**Step 4: Remove dead_code annotations**

In `src/terraform.rs`, remove the `#[allow(dead_code)]` line above `pub async fn plan()`.

In `src/helm.rs`, remove the `#[allow(dead_code)]` line above `pub async fn status()`.

**Step 5: Update UI tests**

In `tests/ui_tests.rs`, add to the callback wiring section:

```rust
ui.on_terraform_plan(|| {});
ui.on_helm_status(|| {});
```

Add to the callback invocation section:

```rust
// terraform_plan
let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
let c = counter.clone();
ui.on_terraform_plan(move || { c.set(c.get() + 1); });
ui.invoke_terraform_plan();
assert_eq!(counter.get(), 1);

// helm_status
let counter = std::rc::Rc::new(std::cell::Cell::new(0u32));
let c = counter.clone();
ui.on_helm_status(move || { c.set(c.get() + 1); });
ui.invoke_helm_status();
assert_eq!(counter.get(), 1);
```

**Step 6: Run tests and build**

Run: `cargo test && cargo clippy`
Expected: All tests pass, zero warnings.

**Step 7: Commit**

```bash
git add src/main.rs src/terraform.rs src/helm.rs ui/app-window.slint ui/components/cluster-panel.slint tests/ui_tests.rs
git commit -m "feat: wire terraform plan and helm status buttons to UI"
```

---

### Task 5: Expand Settings Tab

**Files:**
- Modify: `ui/app-window.slint` (add settings properties and UI elements)
- Modify: `src/main.rs` (load/save all config fields)
- Modify: `tests/ui_tests.rs` (test new properties)

**Step 1: Add new properties to AppWindow in app-window.slint**

Add these to the property declarations (after the existing Settings state section):

```slint
// Extended settings state
in-out property <string> claude-mode: "daemon";
in-out property <string> git-user-name: "Claude Code Bot";
in-out property <string> git-user-email: "claude-bot@localhost";
in-out property <string> cpu-limit: "2";
in-out property <string> memory-limit: "4Gi";
in-out property <string> terraform-dir: "terraform";
in-out property <string> helm-chart-dir: "helm/claude-code";
```

**Step 2: Expand the Settings tab UI**

Replace the Settings Tab content in `app-window.slint` with:

```slint
Tab {
    title: "Settings";

    ScrollView {
        VerticalBox {
            padding: 16px;
            spacing: 12px;

            Text {
                text: "Settings";
                font-size: 20px;
                font-weight: 700;
            }

            GroupBox {
                title: "Anthropic API Key";

                LineEdit {
                    text <=> root.api-key;
                    placeholder-text: "sk-ant-...";
                    input-type: password;
                }
            }

            GroupBox {
                title: "Claude Mode";

                HorizontalBox {
                    spacing: 8px;

                    Text {
                        text: "Mode:";
                        vertical-alignment: center;
                        font-size: 13px;
                    }

                    ComboBox {
                        model: ["daemon", "headless"];
                        current-value <=> root.claude-mode;
                    }
                }
            }

            GroupBox {
                title: "Git Configuration";

                VerticalBox {
                    spacing: 8px;

                    HorizontalBox {
                        spacing: 8px;
                        Text { text: "User Name:"; width: 100px; vertical-alignment: center; font-size: 13px; }
                        LineEdit { text <=> root.git-user-name; placeholder-text: "Claude Code Bot"; }
                    }

                    HorizontalBox {
                        spacing: 8px;
                        Text { text: "Email:"; width: 100px; vertical-alignment: center; font-size: 13px; }
                        LineEdit { text <=> root.git-user-email; placeholder-text: "claude-bot@localhost"; }
                    }
                }
            }

            GroupBox {
                title: "Resource Limits";

                VerticalBox {
                    spacing: 8px;

                    HorizontalBox {
                        spacing: 8px;
                        Text { text: "CPU Limit:"; width: 100px; vertical-alignment: center; font-size: 13px; }
                        LineEdit { text <=> root.cpu-limit; placeholder-text: "2"; }
                    }

                    HorizontalBox {
                        spacing: 8px;
                        Text { text: "Memory:"; width: 100px; vertical-alignment: center; font-size: 13px; }
                        LineEdit { text <=> root.memory-limit; placeholder-text: "4Gi"; }
                    }
                }
            }

            GroupBox {
                title: "Directories";

                VerticalBox {
                    spacing: 8px;

                    HorizontalBox {
                        spacing: 8px;
                        Text { text: "Terraform:"; width: 100px; vertical-alignment: center; font-size: 13px; }
                        LineEdit { text <=> root.terraform-dir; placeholder-text: "terraform"; }
                    }

                    HorizontalBox {
                        spacing: 8px;
                        Text { text: "Helm Chart:"; width: 100px; vertical-alignment: center; font-size: 13px; }
                        LineEdit { text <=> root.helm-chart-dir; placeholder-text: "helm/claude-code"; }
                    }
                }
            }

            GroupBox {
                title: "Platform Info";

                VerticalBox {
                    spacing: 4px;

                    Text {
                        text: "Detected platform: " + root.platform-name;
                        font-size: 13px;
                    }
                }
            }

            Button {
                text: "Save Settings";
                clicked => { root.save-settings(); }
            }

            // Spacer
            Rectangle { vertical-stretch: 1; }
        }
    }
}
```

Note: This requires adding `ScrollView` and `ComboBox` to the imports at the top of `app-window.slint`:

```slint
import { VerticalBox, HorizontalBox, Button, LineEdit, GroupBox, TabWidget, ScrollView, ComboBox } from "std-widgets.slint";
```

**Step 3: Load initial values in main.rs**

In the "Set initial UI state" block in `main.rs`, add after the existing property sets:

```rust
ui.set_claude_mode(s.config.claude_mode.clone().into());
ui.set_git_user_name(s.config.git_user_name.clone().into());
ui.set_git_user_email(s.config.git_user_email.clone().into());
ui.set_cpu_limit(s.config.cpu_limit.clone().into());
ui.set_memory_limit(s.config.memory_limit.clone().into());
ui.set_terraform_dir(s.config.terraform_dir.clone().into());
ui.set_helm_chart_dir(s.config.helm_chart_dir.clone().into());
```

**Step 4: Update save_settings in main.rs**

Replace the `on_save_settings` closure body to read all fields:

```rust
ui.on_save_settings(move || {
    if let Some(ui) = ui_handle.upgrade() {
        let mut s = state.lock().unwrap();
        let key = ui.get_api_key().to_string();
        s.config.api_key = if key.is_empty() { None } else { Some(key) };
        s.config.claude_mode = ui.get_claude_mode().to_string();
        s.config.git_user_name = ui.get_git_user_name().to_string();
        s.config.git_user_email = ui.get_git_user_email().to_string();
        s.config.cpu_limit = ui.get_cpu_limit().to_string();
        s.config.memory_limit = ui.get_memory_limit().to_string();
        s.config.terraform_dir = ui.get_terraform_dir().to_string();
        s.config.helm_chart_dir = ui.get_helm_chart_dir().to_string();
        match s.config.save() {
            Ok(_) => s.append_log("Settings saved."),
            Err(e) => s.append_log(&format!("Failed to save settings: {}", e)),
        }
    }
});
```

**Step 5: Update UI tests**

In `tests/ui_tests.rs`, add default checks and set/get tests for the new properties:

```rust
// Check defaults for new settings properties
assert_eq!(ui.get_claude_mode(), "daemon");
assert_eq!(ui.get_git_user_name(), "Claude Code Bot");
assert_eq!(ui.get_git_user_email(), "claude-bot@localhost");
assert_eq!(ui.get_cpu_limit(), "2");
assert_eq!(ui.get_memory_limit(), "4Gi");
assert_eq!(ui.get_terraform_dir(), "terraform");
assert_eq!(ui.get_helm_chart_dir(), "helm/claude-code");

// Set/get new settings properties
ui.set_claude_mode("headless".into());
assert_eq!(ui.get_claude_mode(), "headless");
ui.set_git_user_name("Test Bot".into());
assert_eq!(ui.get_git_user_name(), "Test Bot");
ui.set_git_user_email("test@example.com".into());
assert_eq!(ui.get_git_user_email(), "test@example.com");
ui.set_cpu_limit("4".into());
assert_eq!(ui.get_cpu_limit(), "4");
ui.set_memory_limit("8Gi".into());
assert_eq!(ui.get_memory_limit(), "8Gi");
ui.set_terraform_dir("custom-tf".into());
assert_eq!(ui.get_terraform_dir(), "custom-tf");
ui.set_helm_chart_dir("custom-helm".into());
assert_eq!(ui.get_helm_chart_dir(), "custom-helm");
```

**Step 6: Run tests and build**

Run: `cargo test && cargo clippy`
Expected: All tests pass, zero warnings.

**Step 7: Commit**

```bash
git add ui/app-window.slint src/main.rs tests/ui_tests.rs
git commit -m "feat: expand settings tab with all config fields"
```

---

### Task 6: Wire exec_claude() to UI

**Files:**
- Modify: `ui/components/pods-panel.slint` (add Claude button and prompt input area)
- Modify: `ui/app-window.slint` (add callbacks and properties)
- Modify: `src/main.rs` (wire callbacks)
- Modify: `src/kubectl.rs` (remove `#[allow(dead_code)]`)
- Modify: `tests/ui_tests.rs` (add callback tests)

**Step 1: Add properties and callbacks to app-window.slint**

Add properties:

```slint
// Claude exec state
in-out property <string> claude-prompt: "";
in property <string> claude-target-pod: "";
```

Add callbacks (in Pods callbacks section):

```slint
callback exec-claude(int);
callback send-prompt(string);
```

**Step 2: Update PodsPanel in pods-panel.slint**

Add new callbacks and properties to the component:

```slint
callback exec-claude(int);
callback send-prompt(string);
in-out property <string> claude-prompt: "";
in property <string> claude-target-pod: "";
```

Add a "Claude" button in each pod row's action buttons (after the "Logs" button, before "Delete"):

```slint
Button {
    text: "Claude";
    height: 28px;
    clicked => { exec-claude(idx); }
}
```

Add a prompt input area below the ScrollView (before the Summary HorizontalBox):

```slint
// Claude prompt input (visible when a target pod is selected)
if claude-target-pod != "": HorizontalBox {
    spacing: 8px;
    height: 36px;

    Text {
        text: "Prompt for " + claude-target-pod + ":";
        font-size: 12px;
        vertical-alignment: center;
        color: #666;
    }

    LineEdit {
        text <=> claude-prompt;
        placeholder-text: "Enter a prompt for Claude...";
        horizontal-stretch: 1;
    }

    Button {
        text: "Send";
        enabled: claude-prompt != "";
        clicked => { send-prompt(claude-prompt); }
    }
}
```

**Step 3: Wire PodsPanel callbacks in app-window.slint**

In the Pods Tab section, add:

```slint
exec-claude(idx) => { root.exec-claude(idx); }
send-prompt(prompt) => { root.send-prompt(prompt); }
claude-prompt <=> root.claude-prompt;
claude-target-pod: root.claude-target-pod;
```

**Step 4: Wire callbacks in main.rs**

Add exec_claude callback (after the view_logs block):

```rust
{
    let ui_handle = ui.as_weak();
    let state = state.clone();

    ui.on_exec_claude(move |idx| {
        let s = state.lock().unwrap();
        let pod_name = s
            .pods
            .get(idx as usize)
            .map(|p| p.name.clone())
            .unwrap_or_default();

        if let Some(ui) = ui_handle.upgrade() {
            ui.set_claude_target_pod(pod_name.into());
            ui.set_claude_prompt("".into());
        }
    });
}
```

Add send_prompt callback:

```rust
{
    let ui_handle = ui.as_weak();
    let state = state.clone();
    let rt_handle = rt.handle().clone();

    ui.on_send_prompt(move |prompt| {
        let ui = ui_handle.clone();
        let state = state.clone();
        let prompt = prompt.to_string();

        let (kubectl, pod_name) = {
            let s = state.lock().unwrap();
            let pod_name = if let Some(u) = ui.upgrade() {
                u.get_claude_target_pod().to_string()
            } else {
                String::new()
            };
            (s.kubectl_runner(), pod_name)
        };

        if pod_name.is_empty() {
            return;
        }

        set_busy(&ui, true);
        append_log(&state, &format!("Sending prompt to pod {}...", pod_name));
        sync_log(&ui, &state);

        rt_handle.spawn(async move {
            let result = kubectl.exec_claude(&pod_name, &prompt).await;

            slint::invoke_from_event_loop(move || {
                match result {
                    Ok(r) => {
                        append_log(
                            &state,
                            &format!("--- Claude response from {} ---\n{}", pod_name, r.stdout),
                        );
                        if !r.stderr.is_empty() {
                            append_log(&state, &format!("STDERR: {}", r.stderr.trim()));
                        }
                    }
                    Err(e) => append_log(&state, &format!("Claude exec error: {}", e)),
                }
                // Clear prompt and target after sending
                if let Some(ui) = ui.upgrade() {
                    ui.set_claude_prompt("".into());
                    ui.set_claude_target_pod("".into());
                }
                set_busy(&ui, false);
                sync_log(&ui, &state);
            })
            .ok();
        });
    });
}
```

**Step 5: Remove dead_code annotation from exec_claude**

In `src/kubectl.rs`, remove the `#[allow(dead_code)]` line above `pub async fn exec_claude()`.

**Step 6: Update UI tests**

In `tests/ui_tests.rs`, add:

Default checks:
```rust
assert_eq!(ui.get_claude_prompt(), "");
assert_eq!(ui.get_claude_target_pod(), "");
```

Set/get tests:
```rust
ui.set_claude_prompt("test prompt".into());
assert_eq!(ui.get_claude_prompt(), "test prompt");
ui.set_claude_target_pod("my-pod-123".into());
assert_eq!(ui.get_claude_target_pod(), "my-pod-123");
```

Callback wiring:
```rust
ui.on_exec_claude(|_idx| {});
ui.on_send_prompt(|_prompt| {});
```

Callback invocation:
```rust
// exec_claude
let received_idx = std::rc::Rc::new(std::cell::Cell::new(-1i32));
let i = received_idx.clone();
ui.on_exec_claude(move |idx| { i.set(idx); });
ui.invoke_exec_claude(2);
assert_eq!(received_idx.get(), 2);

// send_prompt
let received_prompt = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
let p = received_prompt.clone();
ui.on_send_prompt(move |prompt| { *p.borrow_mut() = prompt.to_string(); });
ui.invoke_send_prompt("hello claude".into());
assert_eq!(*received_prompt.borrow(), "hello claude");
```

**Step 7: Run tests and build**

Run: `cargo test && cargo clippy`
Expected: All tests pass, zero warnings.

**Step 8: Commit**

```bash
git add src/main.rs src/kubectl.rs ui/app-window.slint ui/components/pods-panel.slint tests/ui_tests.rs
git commit -m "feat: wire exec_claude to UI with per-pod prompt input"
```

---

### Task 7: Create README.md

**Files:**
- Create: `README.md`

**Step 1: Write the README**

```markdown
# Claude in K3s

A cross-platform Rust GUI application for managing [Claude Code](https://docs.anthropic.com/en/docs/claude-code) instances running as pods in a local [K3s](https://k3s.io/) Kubernetes cluster.

## Architecture

```
┌─────────────────────────────────────┐
│          Slint GUI (4 tabs)         │
│  Cluster │ Projects │ Pods │ Settings│
└────┬─────┴────┬─────┴──┬──┴────┬───┘
     │          │        │       │
     ▼          ▼        ▼       ▼
 Terraform   Docker   Helm   kubectl
  (k3s)     (images)  (deploy) (pods)
     │          │        │       │
     └──────────┴────────┴───────┘
                 │
            K3s Cluster
```

- **Cluster tab** — Terraform lifecycle (init/apply/plan/destroy) and Helm status
- **Projects tab** — Scan a directory for projects, select base images, build Docker images, deploy via Helm
- **Pods tab** — Monitor running pods, view logs, send prompts to Claude, delete pods
- **Settings tab** — API key, Claude mode, git config, resource limits, directories

## Prerequisites

- [Rust](https://rustup.rs/) (1.70+)
- [K3s](https://k3s.io/) installed on the host
- [Terraform](https://www.terraform.io/) (>= 1.5.0)
- [Helm](https://helm.sh/) (v3)
- [Docker](https://www.docker.com/)
- An [Anthropic API key](https://console.anthropic.com/)
- Linux: development headers for your display server (`libxkbcommon-dev`, etc.)

## Build

```bash
# Debug build
cargo build

# Release build (optimized, stripped)
cargo build --release

# Run tests
cargo test

# Lint
cargo clippy
```

## Usage

```bash
# Run the application
cargo run

# Or run the release binary
./target/release/claude-in-k3s
```

### First-time setup

1. **Settings tab**: Enter your Anthropic API key and click Save
2. **Cluster tab**: Click Init → Apply to start the K3s cluster via Terraform
3. **Projects tab**: Browse to a directory containing your projects
4. Select projects and choose base images (Node, Python, Rust, Go, .NET, or custom Dockerfile)
5. Click Launch to build Docker images and deploy pods via Helm
6. **Pods tab**: Monitor pods, view logs, or send prompts to Claude

## Configuration

Settings are stored in `~/.config/claude-in-k3s/config.toml`:

| Field | Default | Description |
|-------|---------|-------------|
| `api_key` | — | Anthropic API key |
| `projects_dir` | — | Directory to scan for projects |
| `claude_mode` | `daemon` | Pod mode: `daemon` (persistent) or `headless` (run and exit) |
| `git_user_name` | `Claude Code Bot` | Git user name for commits inside pods |
| `git_user_email` | `claude-bot@localhost` | Git email for commits inside pods |
| `cpu_limit` | `2` | CPU limit per pod |
| `memory_limit` | `4Gi` | Memory limit per pod |
| `terraform_dir` | `terraform` | Path to Terraform configuration |
| `helm_chart_dir` | `helm/claude-code` | Path to Helm chart |

## Project Structure

```
├── src/
│   ├── main.rs          # Entry point, UI callbacks
│   ├── app.rs           # AppState, runner factories
│   ├── config.rs        # TOML config load/save
│   ├── error.rs         # Error types
│   ├── platform.rs      # Platform detection
│   ├── projects.rs      # Project scanning, base image detection
│   ├── terraform.rs     # Terraform runner
│   ├── helm.rs          # Helm runner
│   ├── kubectl.rs       # Kubectl runner
│   └── docker.rs        # Docker image builder
├── ui/
│   ├── app-window.slint # Root window
│   └── components/      # Panel components
├── helm/claude-code/    # Helm chart
├── terraform/           # Terraform config for k3s
├── docker/              # Dockerfile template + entrypoint
└── tests/               # Integration tests
```

## License

MIT
```

**Step 2: Commit**

```bash
git add README.md
git commit -m "docs: add project README"
```

---

### Final Verification

After all tasks are complete:

**Step 1: Full build and test**

```bash
cargo build && cargo test && cargo clippy
```

Expected: Clean build, all tests pass, zero warnings.

**Step 2: Terraform validation**

```bash
terraform -chdir=terraform fmt -check
```

Expected: No formatting issues.

**Step 3: Helm lint**

```bash
helm lint helm/claude-code
```

Expected: Clean lint.
