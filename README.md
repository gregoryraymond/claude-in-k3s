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
