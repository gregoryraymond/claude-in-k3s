# Completion Gaps Design

Date: 2026-03-02

## Overview

Complete all remaining gaps identified in the project assessment:
7 items ranging from UI wiring to cleanup to documentation.

## 1. Wire exec_claude() to UI

**Goal**: Let users send prompts to Claude running inside a pod.

- Add "Claude" button per pod row in `pods-panel.slint` (next to Logs/Delete)
- Add prompt input area below the pods table: LineEdit + Send button, visible when a pod is selected
- New Slint properties: `claude-prompt` (in-out string), `claude-target-pod` (string), `claude-target-idx` (int)
- New callbacks: `exec-claude(int)` selects target pod, `send-prompt(string)` sends the prompt
- In `main.rs`: `exec-claude` stores the pod name; `send-prompt` calls `kubectl.exec_claude()`, appends response to log
- Remove `#[allow(dead_code)]` from `KubectlRunner::exec_claude()`

## 2. Human-friendly Pod Age

**Goal**: Show "2d 3h" instead of "2025-01-15T10:30:00Z".

- Add `fn format_age(timestamp: &str) -> String` in `kubectl.rs`
- Uses `chrono::DateTime::parse_from_rfc3339` to parse, `chrono::Utc::now()` to compute delta
- Format: days/hours/minutes ("2d 3h", "45m", "< 1m")
- Call in `get_pods()` when building `PodStatus`
- Update existing tests to expect formatted ages

## 3. Expand Settings Tab

**Goal**: Expose all `AppConfig` fields in the UI.

New GroupBoxes in `app-window.slint` Settings tab:
- **Claude Mode**: ComboBox with daemon/headless options
- **Git Configuration**: LineEdits for user name and email
- **Resource Limits**: LineEdits for CPU limit and memory limit
- **Directories**: LineEdits for terraform_dir and helm_chart_dir

New properties on AppWindow:
- `claude-mode`, `git-user-name`, `git-user-email`, `cpu-limit`, `memory-limit`, `terraform-dir`, `helm-chart-dir`

Wire `save-settings` to read all fields. Load initial values on startup.

## 4. Wire plan() and status()

**Goal**: Expose terraform plan and helm status through the UI.

- Add "Plan" button to Terraform Lifecycle group in `cluster-panel.slint`
- Add `callback terraform-plan()` and wire in `main.rs`
- Add "Helm Status" button to cluster panel
- Add `callback helm-status()` and wire in `main.rs`
- Remove `#[allow(dead_code)]` from both methods

## 5. Clean Up Terraform Providers

**Goal**: Remove unused provider declarations.

- Remove `null` and `local` required_providers blocks from `terraform/providers.tf`
- Keep only `required_version = ">= 1.5.0"`

## 6. Create README.md

**Goal**: Document the project for users.

Sections: project description, architecture, prerequisites, build, usage, configuration reference.

## 7. Clean Up Unused Test Helpers

**Goal**: Remove dead test helper code.

- Delete `tests/mock_bin.rs` (all 4 public functions are unused)
