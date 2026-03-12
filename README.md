<div align="center">

# Claude in K3s

### Sandboxed AI coding agents. No DevOps degree required.

*Ever wanted to sandbox Claude Code in containers but don't have 10 years of DevOps experience? Same. So we built a button for it.*

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-dea584.svg)](https://www.rust-lang.org/)
[![Powered by Slint](https://img.shields.io/badge/GUI-Slint-2379F4.svg)](https://slint.dev/)
[![K3s](https://img.shields.io/badge/Kubernetes-K3s-FFC61C.svg)](https://k3s.io/)

---

![Your Code → Docker Image → K3s Pod](docs/images/flow.png)

*Claude Code runs in an isolated container. Your machine stays clean. Your secrets stay safe.*

</div>

---

## The Problem

You want Claude Code to work on your projects. But giving an AI agent unrestricted access to your filesystem, your SSH keys, your `.env` files, and your entire `$PATH` feels... uncomfortable.

**The "proper" solution** involves Kubernetes, Terraform, Helm charts, Docker builds, kubeconfig files, and a week of YAML debugging.

**This solution** involves clicking a button.

## What This Does

Claude in K3s is a **native desktop app** that spins up a local Kubernetes cluster on your machine and runs Claude Code inside isolated containers — one per project. Each pod gets its own filesystem, its own network namespace, and exactly the permissions you give it.

You get the sandboxing. You skip the 47-page runbook.

---

## Features

<table>
<tr>
<td width="50%">

### One-Click Infrastructure
The app detects missing tools (K3s, Terraform, Helm, Docker) and installs them for you. Terraform provisions the cluster. Helm deploys the pods. You click buttons.

### Smart Project Detection
Point it at a directory. It finds your projects, detects the language (Node, Python, Rust, Go, .NET), picks the right base image, and builds Docker containers automatically.

### Live Pod Management
Watch pods come up in real time. Tail logs with auto-scroll. Send prompts to Claude directly from the app. Delete pods when you're done. Everything in one window.

### Pod Actions
Compact icon buttons on each pod row — view logs, send prompts, expose to network, redeploy, or delete. Tooltips explain each action.

### Network Exposure
One-click Service + Ingress creation. Exposes any pod's HTTP port at `{project}.localhost` for easy browser access.

</td>
<td width="50%">

### Bulk Operations
Select individual pods or "select all", then redeploy, expose, unexpose, or delete in bulk. A floating action bar shows when pods are selected.

### Log Viewer
Auto-scroll toggle, previous-container crash logs, and automatic fallback to `kubectl describe` when containers haven't started yet.

### Redeploy
Re-apply the Helm chart for selected pods without rebuilding Docker images. Useful for picking up config changes quickly.

### Resource Controls
Set CPU and memory limits per pod. Claude gets what you give it — no runaway processes eating your machine.

### Custom Dockerfiles
Need a special setup? Drop a `Dockerfile` in `.claude/` or your project root. The app picks it up and builds from that instead.

### Cross-Platform
Runs on Linux, macOS, and Windows (WSL2). The GUI is native on every platform — no Electron, no browser, no 400MB runtime.

</td>
</tr>
</table>

---

## How It Works

![Architecture](docs/images/architecture.png)

**The stack, simplified:**

| Layer | Tool | What It Does |
|-------|------|-------------|
| **Cluster** | Terraform + K3s | Creates a lightweight Kubernetes cluster on your machine |
| **Images** | Docker | Builds container images with your project code baked in |
| **Deployment** | Helm | Deploys Claude Code pods with your API key and config |
| **Interaction** | kubectl | Streams logs, executes prompts, manages pod lifecycle |
| **You** | This app | Clicks buttons instead of writing YAML |

---

## Quick Start

### 1. Build the app

```bash
# Clone the repo
git clone https://github.com/your-org/claude-in-k3s.git
cd claude-in-k3s

# Build (release mode for smaller, faster binary)
cargo build --release

# Run
./target/release/claude-in-k3s
```

> **Requires:** Rust 1.70+. On Linux you'll also need display server headers (`libxkbcommon-dev`, etc.).

### 2. Let the app handle the rest

On first launch, the **Setup Panel** checks for required tools:

```
  ✓  k3s       v1.31.4     Found
  ✗  terraform              Missing  ← Click "Install All"
  ✗  helm                   Missing  ← and go get coffee
  ✓  docker    v27.4.1     Found
```

The app downloads and installs whatever's missing. No `curl | bash` pipelines to memorize.

### 3. Configure

Head to **Settings** and enter:
- Your **Anthropic API key**
- A **projects directory** (where your code lives)
- Resource limits (optional — defaults to 2 CPU / 4Gi RAM per pod)

### 4. Launch

1. **Cluster tab** → Click **Init**, then **Apply**. Terraform creates your K3s cluster.
2. **Projects tab** → Select projects. Click **Launch**. Docker builds images. Helm deploys pods.
3. **Pods tab** → Watch them come up. Send prompts. Read logs. Ship code.

---

## Pod Management

Once your pods are running, the **Pods tab** provides a full management interface.

### Icon Button Actions

Each pod row has compact icon buttons for quick actions:

| Icon | Action | Description |
|------|--------|-------------|
| Log | **View Logs** | Opens the log viewer for that pod |
| Claude | **Send Prompt** | Opens the Claude prompt interface |
| Network | **Expose** | Creates a Service + Ingress for browser access |
| Redeploy | **Redeploy** | Re-applies the Helm chart without rebuilding |
| Delete | **Delete** | Removes the pod from the cluster |

### Network Exposure

Click the **Expose** button on a pod to:
1. Auto-detect the listening port inside the container (falls back to 8080)
2. Create a Kubernetes **Service** routing traffic to that port
3. Create an **Ingress** rule at `{project}.localhost`

Browse to `http://your-project.localhost` to reach the running app. Click **Unexpose** to tear down the Service and Ingress.

### Selection and Bulk Actions

- Click the checkbox on individual pods, or use **Select All**
- A floating action bar appears showing the count and names of selected pods
- Bulk actions: **Redeploy Selected**, **Expose Selected**, **Unexpose Selected**, **Delete Selected**

### Log Viewer

- Streams the latest logs with configurable tail length
- **Auto-scroll** toggle keeps the view pinned to the bottom
- If the container has restarted, previous container logs are shown above current logs with a separator
- Falls back to `kubectl describe` output when containers haven't started yet

### Redeploy

Redeploy re-runs the Helm upgrade for selected pods without rebuilding Docker images. This is useful when you've changed Helm values (API keys, resource limits, environment variables) and want to pick up those changes quickly.

---

## Configuration

Settings persist in `~/.config/claude-in-k3s/config.toml`:

```toml
api_key = "sk-ant-..."
projects_dir = "/home/you/projects"
claude_mode = "daemon"          # "daemon" (persistent) or "headless" (run and exit)

git_user_name = "Claude Code Bot"
git_user_email = "claude-bot@localhost"

cpu_limit = "2"
memory_limit = "4Gi"

terraform_dir = "terraform"
helm_chart_dir = "helm/claude-code"
```

---

## Project Structure

```
claude-in-k3s/
├── src/
│   ├── main.rs            Entry point, UI event loop, callbacks
│   ├── app.rs             Central app state and runner factories
│   ├── config.rs          TOML configuration management
│   ├── deps.rs            Dependency detection and auto-install
│   ├── docker.rs          Docker image builder
│   ├── error.rs           Error types and command results
│   ├── health.rs          Background health-check polling
│   ├── helm.rs            Helm chart deployment
│   ├── kubectl.rs         Pod management and Claude interaction
│   ├── platform.rs        Cross-platform detection (Linux/macOS/WSL2/Windows)
│   ├── projects.rs        Project scanning and language detection
│   ├── recovery.rs        Cluster failure diagnosis and auto-recovery
│   ├── terraform.rs       Terraform lifecycle management
│   └── tray.rs            System tray integration
│
├── ui/
│   ├── app-window.slint           Root window with tab navigation
│   ├── icons/                     SVG icons for pod actions and navigation
│   └── components/
│       ├── cluster-panel.slint    Terraform + Helm controls
│       ├── icon-button.slint      Reusable icon button with tooltip
│       ├── log-viewer.slint       Log display with auto-scroll toggle
│       ├── projects-panel.slint   Project browser and launcher
│       ├── pods-panel.slint       Pod monitor, actions, and bulk operations
│       ├── settings-panel.slint   Configuration form
│       └── setup-panel.slint     First-run dependency installer
│
├── helm/claude-code/       Helm chart for Claude Code pods
├── terraform/              Terraform configs for K3s provisioning
├── docker/                 Dockerfile template + entrypoint script
└── tests/                  Integration and UI tests
```

---

## Supported Languages

The app auto-detects your project's language and selects the right base image:

| Detected File | Language | Base Image |
|--------------|----------|------------|
| `package.json` | Node.js | `node:22-bookworm-slim` |
| `Cargo.toml` | Rust | `rust:1.83-bookworm-slim` |
| `go.mod` | Go | `golang:1.23-bookworm` |
| `requirements.txt` | Python | `python:3.12-slim-bookworm` |
| `*.csproj` | .NET | `mcr.microsoft.com/dotnet/sdk:9.0` |
| *(none)* | Base | `debian:bookworm-slim` |

Need something else? Add a `Dockerfile` to your project's `.claude/` directory or project root.

---

## Why K3s?

[K3s](https://k3s.io/) is a certified Kubernetes distribution that runs in ~512MB of RAM. It's a single binary. It starts in seconds. It gives you real container isolation without the overhead of a full Kubernetes cluster.

**Compared to alternatives:**

| Approach | Isolation | Setup Complexity | Resource Overhead |
|----------|-----------|-----------------|-------------------|
| Run Claude on bare metal | None | Trivial | None |
| Docker containers | Process-level | Moderate | Low |
| **K3s pods (this project)** | **Namespace + cgroup** | **One click** | **Low** |
| Full Kubernetes (EKS, GKE) | Full | High | High |

You get real Kubernetes-grade isolation. On your laptop. Without the cloud bill.

---

## FAQ

<details>
<summary><strong>Is this production-ready?</strong></summary>

This is designed for local development sandboxing, not production deployment. It's great for safely letting Claude work on your projects without giving it the keys to your kingdom.
</details>

<details>
<summary><strong>Does Claude have internet access inside the pod?</strong></summary>

By default, pods have outbound internet access (needed for API calls). Network policies can be configured via the Helm chart if you need tighter restrictions.
</details>

<details>
<summary><strong>Can I run multiple Claude instances at once?</strong></summary>

Yes. Each project gets its own pod. Select multiple projects in the Projects tab and launch them all at once.
</details>

<details>
<summary><strong>What happens to my code?</strong></summary>

Your project code is copied into the Docker image at build time. Changes Claude makes stay inside the container. Nothing writes back to your host filesystem unless you explicitly extract it.
</details>

<details>
<summary><strong>Why Rust?</strong></summary>

A native binary with no runtime dependencies. The release build is small, fast, and starts instantly. No "please install Node 18 and Python 3.11 and also somehow Java" prerequisites.
</details>

---

## Development

```bash
# Run in debug mode
cargo run

# Run tests
cargo test

# Lint
cargo clippy

# Format
cargo fmt
```

---

## License

[MIT](LICENSE) — Do whatever you want with it.

---

<div align="center">

**Built with Rust, Slint, and a mass disdain for uncontained AI agents.**

*If Claude is going to rewrite your codebase, it should at least do it in a sandbox.*

</div>
