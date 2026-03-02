# Auto-Install Missing Dependencies Design

Date: 2026-03-02

## Overview

On app startup, check if required tools (k3s, terraform, helm, docker) are installed. If any are missing, show a setup screen with install buttons. Uses direct binary downloads for terraform/helm and official install scripts for k3s/docker.

## Tool Detection

New module `src/deps.rs`:

- `ToolStatus` enum: `Found { version: String }` or `Missing`
- `DepsStatus` struct with status for each of the 4 tools
- `check_tool(binary: &str)` — runs `which <binary>` to check presence, `<binary> version` to get version string
- `check_all(platform: &Platform)` — checks all 4 tools, returns `DepsStatus`
- All checks are synchronous (fast `which` calls)

## Installation Strategy

| Tool | Method | Linux/WSL2 | macOS |
|------|--------|------------|-------|
| k3s | Install script | `curl -sfL https://get.k3s.io \| sh -` | Not available (show message) |
| terraform | Binary download | Download to `~/.local/bin/` | Download to `~/.local/bin/` |
| helm | Binary download | Download to `~/.local/bin/` | Download to `~/.local/bin/` |
| docker | Install script | `curl -fsSL https://get.docker.com \| sh` | Show Docker Desktop instructions |

Binary download flow for terraform/helm:
1. Detect architecture (x86_64/aarch64)
2. Download tarball/zip from official release URL
3. Extract binary to `~/.local/bin/`
4. Ensure `~/.local/bin` is on PATH (warn if not)

Install script flow for k3s/docker:
1. Run official install script via `sh -c "curl ... | sh"`
2. These require sudo — the script will prompt for password in terminal
3. Log output shown in the setup screen

## UI Flow

1. `AppState::new()` calls `deps::check_all()` during initialization
2. `DepsStatus` stored in `AppState`
3. Main UI checks if all deps are met
4. If missing: show setup panel with per-tool status (checkmark/X + version)
5. "Install Missing" button triggers async installation
6. Progress/log shown in text area
7. After install: re-check, enable "Continue" when all found

## New UI Component: SetupPanel

In `ui/components/setup-panel.slint`:
- Title: "Setup — Missing Dependencies"
- List of 4 tools with status indicators (green check / red X)
- Version string shown when found
- "Install Missing" button
- Log output area for install progress
- "Continue" button (enabled only when all deps found)

The setup panel replaces the TabWidget when deps are missing. When all deps are satisfied, the TabWidget is shown.

## Architecture Integration

- `src/deps.rs` — new module for detection + installation
- `src/platform.rs` — add `detect_arch()` function for download URLs
- `src/app.rs` — add `DepsStatus` to `AppState`
- `src/main.rs` — check deps on startup, show setup panel or main UI
- `ui/app-window.slint` — conditional: show setup panel or tab widget
- `ui/components/setup-panel.slint` — new component

## Platform Notes

- Windows: Show "manual install required" for all tools, provide URLs
- macOS: k3s not available, docker needs Docker Desktop — show instructions for those, binary download for terraform/helm
- Linux/WSL2: Full auto-install support for all 4 tools
