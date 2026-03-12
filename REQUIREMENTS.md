# Requirements Specification — Claude in K3s

## Conventions

- **SHALL** indicates a mandatory requirement.
- Each requirement asserts exactly one verifiable behavior.
- IDs use the pattern `{area}.{number}` where area maps to a functional panel or cross-cutting concern.

---

## 1 — Cluster Panel

### 1.1 Infrastructure Visibility

| ID | Requirement |
|----|-------------|
| CLU-1 | The Cluster panel SHALL display the current health status of the Docker daemon (Linux) or Docker Desktop (Windows). |
| CLU-2 | The Cluster panel SHALL display the current health status of the Kubernetes cluster (k3d on Windows, k3s on Linux). |
| CLU-3 | The Cluster panel SHALL display the current health status of Helm. |
| CLU-4 | The Cluster panel SHALL display the current health status of WSL when running on Windows. |
| CLU-5 | The Cluster panel SHALL display the current status of each deployed workload pod as a color-coded tile (green = healthy, yellow = pending, red = error). |
| CLU-6 | The Cluster panel SHALL display cluster memory usage as a text label showing used and total memory. |
| CLU-7 | The Cluster panel SHALL display cluster memory usage as a percentage bar. |
| CLU-8 | The Cluster panel SHALL color the memory bar green when usage is below 70%, yellow between 70–90%, and red above 90%. |
| CLU-9 | The Cluster panel SHALL display the Kubernetes cluster version. |
| CLU-10 | The Cluster panel SHALL display the Kubernetes cluster uptime. |
| CLU-11 | The Cluster panel SHALL display the number of active Helm releases. |
| CLU-12 | When the Docker daemon is not reachable, the Cluster panel SHALL display a diagnostic message identifying the probable cause (e.g., "Docker daemon not running", "Permission denied — add user to docker group"). |
| CLU-34 | The Cluster panel SHALL auto-refresh all health checks (Docker, K8s, Helm, WSL, memory) every 10 seconds. |

### 1.2 Cluster Provisioning

| ID | Requirement |
|----|-------------|
| CLU-13 | The Cluster panel SHALL provide a "Deploy" button that initiates cluster provisioning. |
| CLU-14 | On Linux, cluster provisioning SHALL use Terraform with k3s. |
| CLU-15 | On Windows, cluster provisioning SHALL use k3d directly without requiring Terraform. |

### 1.3 Automatic Recovery

| ID | Requirement |
|----|-------------|
| CLU-16 | When the Deploy button is activated and Docker Desktop is not running (Windows), the application SHALL attempt to restart Docker Desktop. |
| CLU-17 | When the Deploy button is activated and WSL is unresponsive (Windows), the application SHALL attempt to restart WSL. |
| CLU-18 | When the Deploy button detects a stale Helm release, the application SHALL attempt to re-apply it. |
| CLU-19 | When the Deploy button detects a corrupted k3d cluster, the application SHALL attempt to recreate it. |
| CLU-20 | The application SHALL limit automatic recovery retry attempts to a maximum of 2 per operation type before stopping. |
| CLU-29 | When Kubernetes failures occur, the application SHALL offer guided manual fixes if automatic remediation fails. |
| CLU-30 | The application SHALL actively monitor cluster connectivity during operations and abort immediately if the cluster becomes unreachable. |
| CLU-31 | If the cluster becomes unreachable during an operation, the application SHALL trigger the automatic recovery flow (CLU-16 through CLU-19). |
| CLU-32 | If automatic recovery restores the cluster during an interrupted operation, the application SHALL resume the interrupted operation. |
| CLU-33 | On Linux, if Terraform state is corrupted or out of sync, the application SHALL delete the state file and reinitialize from scratch. |

### 1.4 Advanced View

| ID | Requirement |
|----|-------------|
| CLU-21 | The Cluster panel SHALL provide an "Advanced" toggle that reveals manual infrastructure controls. |
| CLU-22 | The advanced view SHALL provide a "Restart WSL" button, visible only on Windows. |
| CLU-23 | The advanced view SHALL provide a "Restart Docker" button. |
| CLU-24 | The advanced view SHALL provide a "Recreate Cluster" button. |
| CLU-25 | The advanced view SHALL provide a Terraform "Plan" button, enabled only when Terraform has been initialized. |
| CLU-26 | The advanced view SHALL provide a Terraform "Destroy" button, enabled only when Terraform has been initialized. |
| CLU-27 | The advanced view SHALL provide a Helm "Status" button. |
| CLU-28 | The advanced view SHALL display a log viewer showing output from all cluster operations performed during the current session. |

---

## 2 — Projects Panel

### 2.1 Folder Selection and Persistence

| ID | Requirement |
|----|-------------|
| PRJ-1 | The Projects panel SHALL allow the user to browse and select a projects root directory. |
| PRJ-2 | The Projects panel SHALL persist the selected projects directory across application restarts. |
| PRJ-3 | The Projects panel SHALL provide a "Refresh" button to rescan the selected directory. |
| PRJ-42 | The default project discovery directory SHALL be `~/repos`. |

### 2.2 Project Discovery

| ID | Requirement |
|----|-------------|
| PRJ-4 | The Projects panel SHALL list all immediate subdirectories of the selected root directory as projects. |
| PRJ-5 | The Projects panel SHALL skip hidden directories (names starting with `.`) during scanning. |
| PRJ-6 | The Projects panel SHALL display projects in alphabetical order. |
| PRJ-43 | The application SHALL use a filesystem watcher to detect new projects appearing in the projects directory. |
| PRJ-58 | If the filesystem watcher fails or stops receiving events, the application SHALL fall back to polling the projects directory every 30 seconds. |
| PRJ-44 | If a project directory is deleted while its pod is deployed, the application SHALL mark the project as missing but keep its deployment running. |

### 2.3 Language Detection and Base Image

| ID | Requirement |
|----|-------------|
| PRJ-7 | The application SHALL auto-detect a Node.js project when `package.json` is present and assign the `node:22-bookworm-slim` base image. |
| PRJ-8 | The application SHALL auto-detect a Rust project when `Cargo.toml` is present and assign the `rust:1.83-slim-bookworm` base image. |
| PRJ-9 | The application SHALL auto-detect a Go project when `go.mod` is present and assign the `golang:1.23-bookworm` base image. |
| PRJ-10 | The application SHALL auto-detect a Python project when `requirements.txt` is present and assign the `python:3.12-slim-bookworm` base image. |
| PRJ-11 | The application SHALL auto-detect a Python project when `pyproject.toml` is present and assign the `python:3.12-slim-bookworm` base image. |
| PRJ-12 | The application SHALL auto-detect a Python project when `setup.py` is present and assign the `python:3.12-slim-bookworm` base image. |
| PRJ-13 | The application SHALL auto-detect a .NET project when a `.csproj` file is present and assign the `mcr.microsoft.com/dotnet/sdk:9.0` base image. |
| PRJ-14 | The application SHALL auto-detect a .NET project when a `.sln` file is present and assign the `mcr.microsoft.com/dotnet/sdk:9.0` base image. |
| PRJ-15 | The application SHALL fall back to the `debian:bookworm-slim` base image when no language marker file is found. |
| PRJ-61 | If multiple language marker files are detected in the same project, the application SHALL display an ambiguity warning and require the user to select the base image via the combo box. |
| PRJ-16 | The Projects panel SHALL detect a custom Dockerfile in the project root and mark the project with a "has Dockerfile" indicator. |
| PRJ-17 | The Projects panel SHALL detect a custom Dockerfile in the project's `.claude/` subdirectory and mark the project with a "has Dockerfile" indicator. |
| PRJ-18 | The Projects panel SHALL allow the user to override the auto-detected base image via a combo box. |

### 2.4 Selection and Launch

| ID | Requirement |
|----|-------------|
| PRJ-19 | The Projects panel SHALL allow the user to select multiple undeployed projects via individual checkboxes. |
| PRJ-20 | The Projects panel SHALL disable checkboxes for already-deployed projects. |
| PRJ-21 | The Projects panel SHALL provide a "Select All / Deselect All" toggle that applies only to undeployed projects. |
| PRJ-22 | The Projects panel SHALL provide a "Launch Selected" button that builds Docker images for all selected projects in parallel. |
| PRJ-23 | After building images, the application SHALL deploy Helm charts for all selected projects. |
| PRJ-24 | Each selected project SHALL be deployed into its own Kubernetes namespace named `claude-{sanitized-project-name}`. |
| PRJ-59 | Namespace sanitization SHALL lowercase the project name, replace non-alphanumeric characters with hyphens, and truncate to fit the 63-character Kubernetes namespace limit. |
| PRJ-60 | If two projects sanitize to the same namespace name, the application SHALL append a numeric suffix to the second project (e.g., `claude-my-app-2`). |
| PRJ-25 | Each selected project SHALL be launched as an independent pod in the cluster. |
| PRJ-26 | The Docker image built for each project SHALL include Claude Code as a pre-installed tool. |
| PRJ-27 | When a project is re-launched, the application SHALL rebuild the Docker image. |
| PRJ-28 | When a custom Dockerfile is detected, the application SHALL build it first and then layer `Dockerfile.claude-overlay` on top to add Claude Code. |
| PRJ-29 | The application SHALL generate a Helm chart per project and persist it in the platform app data directory. |
| PRJ-30 | When total requested resources exceed cluster capacity, the application SHALL prompt the user to select which projects to deploy. |
| PRJ-45 | Generated Helm charts SHALL include explicit version numbers. |
| PRJ-46 | Deployment updates SHALL automatically choose between Helm upgrade and reinstall based on failure state. |
| PRJ-47 | A project SHALL be considered running successfully when its container enters the running state. |

### 2.5 Container Mounts

| ID | Requirement |
|----|-------------|
| PRJ-31 | If the host's `~/.claude` directory does not exist, the application SHALL create it before mounting. |
| PRJ-56 | Each launched pod SHALL mount the host's `~/.claude` directory into `~/.claude` inside the container. |
| PRJ-32 | Each launched pod SHALL mount the project's source directory from the host into `/workspace` inside the container. |
| PRJ-33 | Each launched pod SHALL mount any extra volume mounts configured in Settings (SET-12). |
| PRJ-57 | Before launching, the application SHALL validate that all configured extra mount host paths exist and block the launch with an error if any path is invalid. |
| PRJ-34 | The container's working directory (WORKDIR) SHALL be set to `/workspace`. |
| PRJ-35 | The git user name from Settings (SET-4) SHALL be injected into the container's git config at launch. |
| PRJ-36 | The git user email from Settings (SET-5) SHALL be injected into the container's git config at launch. |
| PRJ-48 | Project source directories SHALL be live bind-mounted so containers immediately see host file changes. |
| PRJ-49 | Containers SHALL be ephemeral; no state SHALL persist beyond the mounted host directories. |

### 2.6 Environment Variables

| ID | Requirement |
|----|-------------|
| PRJ-50 | If a `.env` file exists in the project directory, the application SHALL load its values as environment variables in the container. |
| PRJ-51 | Environment variables loaded from `.env` files SHALL be stored as Kubernetes Secrets. |

### 2.7 Build Progress

| ID | Requirement |
|----|-------------|
| PRJ-37 | During launch, the Projects panel SHALL display a per-project tab showing real-time build log output. |
| PRJ-38 | During launch, the Projects panel SHALL display a summary tab with traffic-light step indicators (pending = grey, running = yellow, done = green, failed = red). |
| PRJ-39 | The Projects panel SHALL provide a "Cancel" button that aborts the current build operation. |
| PRJ-52 | When a project's Docker build fails, the application SHALL skip that project and continue building the remaining selected projects. |
| PRJ-53 | When a project's Docker build fails, the summary tab SHALL display the error, a remediation hint, and a "Retry" button for that project. |
| PRJ-54 | When a Helm deploy fails, the application SHALL automatically clean up the built image and any partial Kubernetes resources for that project. |
| PRJ-55 | When a Docker build fails due to insufficient disk space, the application SHALL trigger an immediate image cleanup (IMG-1) and retry the build. |

### 2.8 Deployed State

| ID | Requirement |
|----|-------------|
| PRJ-40 | The Projects panel SHALL visually indicate which projects are currently deployed. |
| PRJ-41 | The Projects panel SHALL provide a "Stop Selected" button that uninstalls the Helm chart for each selected deployed project. |

---

## 3 — Pods Panel

### 3.1 Pod List

| ID | Requirement |
|----|-------------|
| POD-1 | The Pods panel SHALL display all pods deployed by this application in a scrollable table. |
| POD-2 | The Pods panel SHALL display a "Project name" column for each pod. |
| POD-3 | The Pods panel SHALL display a "Status" column for each pod. |
| POD-4 | The Pods panel SHALL display a "Ready" column (Yes/No) for each pod. |
| POD-5 | The Pods panel SHALL display a "Restarts" column for each pod. |
| POD-6 | The Pods panel SHALL display an "Age" column for each pod. |
| POD-7 | The Pods panel SHALL display a "Warnings" column for each pod. |
| POD-8 | The Pods panel SHALL display a "Port" column for each pod. |
| POD-9 | The Pods panel SHALL display an "Actions" column for each pod. |
| POD-10 | The Pods panel SHALL display the pod status as a colored badge (green = Running, yellow = Pending, red = Failed/CrashLoopBackOff). |
| POD-11 | The Pods panel SHALL display the restart count in red when it exceeds 3. |
| POD-12 | The Pods panel SHALL display age in a human-readable format (e.g., "2d 3h", "45m"). |
| POD-13 | The Pods panel SHALL display warning text for pods that have Kubernetes event warnings. |
| POD-14 | The Pods panel SHALL display the exposed container port number when a pod is network-exposed. |
| POD-15 | The Pods panel SHALL display "-" in the port column when a pod is not network-exposed. |
| POD-16 | The Pods panel SHALL auto-refresh the pod list every 10 seconds without user action. |
| POD-17 | The Pods panel SHALL display a total pod count. |

### 3.2 Bulk Selection

| ID | Requirement |
|----|-------------|
| POD-18 | The Pods panel SHALL allow the user to select individual pods via checkboxes. |
| POD-19 | The Pods panel SHALL provide a "Select All" checkbox in the column header. |
| POD-20 | When one or more pods are selected, the Pods panel SHALL display a floating action bar showing the count of selected pods. |
| POD-21 | The floating action bar SHALL display the names of the selected pods. |
| POD-22 | The floating action bar SHALL provide a bulk "Redeploy" button. |
| POD-23 | The floating action bar SHALL provide a bulk "Expose" button. |
| POD-24 | The floating action bar SHALL provide a bulk "Unexpose" button. |
| POD-25 | The floating action bar SHALL provide a bulk "Delete" button. |
| POD-68 | When a bulk action partially fails, the application SHALL skip failed pods, continue with the remaining pods, and display a summary of successes and failures. |

### 3.3 Pod Logging

| ID | Requirement |
|----|-------------|
| POD-26 | The Pods panel SHALL allow the user to view logs for a single pod by clicking a "Logs" icon button. |
| POD-27 | The log viewer SHALL display previous container logs (from crashed containers) above current container logs. |
| POD-28 | The log viewer SHALL separate previous and current container logs with a visual marker. |
| POD-29 | The log viewer SHALL fall back to `kubectl describe` output when the container has not started yet. |
| POD-30 | The log viewer SHALL tail logs in real time with auto-scroll enabled by default. |
| POD-31 | The log viewer SHALL provide an auto-scroll toggle that the user can turn on or off. |
| POD-32 | The log viewer SHALL highlight known failure patterns (e.g., "Warning", "Unhealthy") with distinct coloring. |
| POD-69 | Pod operations (expose, delete, redeploy) SHALL log their output to both the cluster log viewer and the per-pod log viewer. |

### 3.4 Terminal Access

| ID | Requirement |
|----|-------------|
| POD-33 | The Pods panel SHALL provide a "Terminal" icon button that opens a native terminal with a shell session inside the pod. |
| POD-34 | The terminal session SHALL use the platform-native terminal emulator (Windows Terminal or cmd on Windows, Terminal.app on macOS, detected terminal on Linux). |
| POD-35 | When the `kubectl exec` session ends, the terminal window SHALL close. |

### 3.5 Claude Code Integration

| ID | Requirement |
|----|-------------|
| POD-36 | The Pods panel SHALL provide an icon button to launch Claude Code inside the pod in a native terminal. |
| POD-37 | Claude Code launched inside the container SHALL skip the project trust prompt. |
| POD-38 | The Pods panel SHALL provide an icon button to start Claude Code in remote-control mode. |
| POD-39 | Remote-control mode SHALL start only when explicitly initiated by the user via the icon button. |
| POD-40 | When remote-control mode is started, the pod's log output SHALL be tailed to the log viewer. |
| POD-41 | A remote-control session SHALL be accessible from the Claude mobile app and Anthropic API. |
| POD-42 | The remote-control icon button SHALL visually indicate whether a remote-control session is currently active for that pod. |
| POD-59 | Remote-control sessions SHALL persist across pod restarts. |
| POD-60 | The application SHALL automatically restart a remote-control session after a pod restart if one was previously active. |
| POD-66 | If a remote-control session crashes while the pod remains running, the application SHALL automatically restart the remote-control session. |
| POD-67 | The application SHALL limit remote-control session auto-restart attempts to a maximum of 10 before marking the session as failed. |
| POD-61 | Remote Claude agents SHALL initiate and advertise their own remote sessions rather than being discovered by the UI. |

### 3.6 Network Exposure

| ID | Requirement |
|----|-------------|
| POD-43 | The Pods panel SHALL provide an "Expose" icon button that creates a Kubernetes Service for the pod. |
| POD-44 | The "Expose" action SHALL also create a Kubernetes Ingress for the pod. |
| POD-45 | Exposing a pod SHALL auto-detect the listening port inside the container. |
| POD-46 | If port auto-detection fails, the expose action SHALL fall back to port 8080. |
| POD-47 | When a container listens on multiple ports, the expose action SHALL expose all detected ports. |
| POD-48 | After exposing, the pod's web service SHALL be reachable from the host machine at `{project}.localhost`. |
| POD-49 | The Pods panel SHALL provide an "Unexpose" action that removes the Service for the pod. |
| POD-50 | The "Unexpose" action SHALL also remove the Ingress for the pod. |
| POD-51 | If no ports are currently listening inside the container, the expose action SHALL display a warning to the user. |
| POD-52 | The network icon button SHALL visually indicate whether the pod is currently exposed. |
| POD-62 | The application SHALL monitor exposed containers for newly opened ports every 10 seconds and dynamically update the Service and Ingress. |

### 3.7 Pod Lifecycle

| ID | Requirement |
|----|-------------|
| POD-53 | The Pods panel SHALL provide a "Delete" icon button per pod. |
| POD-54 | The delete action SHALL remove the Helm release for the project. |
| POD-55 | The delete action SHALL remove the Kubernetes Service for the project, if one exists. |
| POD-56 | The delete action SHALL remove the Kubernetes Ingress for the project, if one exists. |
| POD-57 | The delete action SHALL delete the project's Kubernetes namespace (`claude-{project}`). |
| POD-58 | The delete action SHALL retain the generated Helm chart in app data for quick relaunch. |
| POD-63 | The bulk "Redeploy" action SHALL re-apply the Helm chart for each selected pod without rebuilding Docker images. |
| POD-64 | All pods deployed by this application SHALL use the Kubernetes `Always` restart policy. |
| POD-65 | When a pod enters CrashLoopBackOff, the application SHALL mark the project as failed in the UI. |

---

## 4 — Settings Panel

| ID | Requirement |
|----|-------------|
| SET-1 | The Settings panel SHALL allow the user to configure the CPU limit per pod (default: "2"). |
| SET-2 | The Settings panel SHALL allow the user to configure the memory limit per pod (default: "4Gi"). |
| SET-3 | The Settings panel SHALL allow the user to configure the cluster memory allocation percentage (default: 80%). |
| SET-4 | The Settings panel SHALL allow the user to configure the git user name (default: "Claude Code Bot"). |
| SET-5 | The Settings panel SHALL allow the user to configure the git user email (default: "claude-bot@localhost"). |
| SET-6 | The Settings panel SHALL allow the user to select the Claude mode ("daemon" or "headless"). |
| SET-7 | The Settings panel SHALL allow the user to configure the Terraform directory path. |
| SET-8 | The Settings panel SHALL allow the user to configure the Helm chart directory path. |
| SET-9 | The Settings panel SHALL display the detected platform name (Linux, macOS, WSL2, or Windows). |
| SET-10 | The Settings panel SHALL provide a "Save" button that persists all settings to `~/.config/claude-in-k3s/config.toml`. |
| SET-11 | All settings SHALL be loaded from the config file on application startup. |
| SET-12 | The Settings panel SHALL allow the user to configure a list of extra volume mounts as host-path to container-path pairs. |
| SET-13 | Extra volume mounts SHALL be persisted in the config file. |
| SET-14 | Extra volume mounts SHALL be applied to every launched pod. |
| SET-15 | The Settings panel SHALL allow the user to configure the Docker image retention period (default: 7 days). |
| SET-16 | The application SHALL NOT support per-project CPU or memory overrides; CPU and memory limits SHALL apply globally to all projects. |
| SET-17 | The Settings panel SHALL allow the user to configure the Docker build timeout (default: 10 minutes). |
| SET-18 | The Settings panel SHALL allow the user to configure the Helm deploy timeout (default: 5 minutes). |
| SET-19 | The Settings panel SHALL validate all inputs on Save and display inline error messages for invalid values. |

---

## 5 — Setup Panel

| ID | Requirement |
|----|-------------|
| SUP-1 | On first launch, the application SHALL display a Setup panel showing the status of each required dependency. |
| SUP-2 | For each dependency, the Setup panel SHALL display whether it is "Found" or "Missing". |
| SUP-3 | For each found dependency, the Setup panel SHALL display its version string. |
| SUP-4 | The Setup panel SHALL provide an "Install Missing" button that downloads and installs all missing dependencies. |
| SUP-5 | The Setup panel SHALL display real-time installation progress in a log viewer. |
| SUP-6 | The Setup panel SHALL provide a "Continue" button that is enabled only when all platform-required dependencies are found. |
| SUP-7 | The Setup panel SHALL be scrollable so all content is accessible on small screens. |
| SUP-8 | The dependency check SHALL be platform-aware: Terraform SHALL be required on Linux but not on Windows. |
| SUP-9 | On subsequent launches, the application SHALL re-check all dependencies before entering the main UI. |
| SUP-10 | If a previously-found dependency is missing on subsequent launch, the application SHALL return to the Setup panel. |

---

## 6 — Cross-Platform

| ID | Requirement |
|----|-------------|
| PLT-1 | The application SHALL run on Linux with native k3s. |
| PLT-2 | The application SHALL run on Windows using k3d over Docker Desktop and WSL2. |
| PLT-3 | The application SHALL auto-detect the host platform at startup and configure tool paths accordingly. |
| PLT-4 | On Windows, the application SHALL translate host filesystem paths to k3d-compatible container mount paths. |

---

## 7 — Container Registry

| ID | Requirement |
|----|-------------|
| REG-1 | On Windows, the application SHALL use the k3d local registry to store built Docker images. |
| REG-2 | Built images SHALL be pushed to the local registry so the cluster can pull them. |
| REG-3 | The local container registry SHALL run inside the k3d cluster. |
| REG-4 | The local registry SHALL operate without authentication. |
| REG-5 | If the local registry is unreachable during an image push, the application SHALL attempt to restart the registry pod and retry the push. |

---

## 8 — Logging

| ID | Requirement |
|----|-------------|
| LOG-1 | The application SHALL write operation logs to persistent storage at the platform-appropriate application data directory (`%APPDATA%/claude-in-k3s/logs/` on Windows, `~/.local/share/claude-in-k3s/logs/` on Linux). |
| LOG-2 | Log files SHALL be named by date so that past sessions are preserved. |
| LOG-3 | Log retention SHALL be capped at a total log directory size of 100 MB. |
| LOG-4 | The logging system SHALL support the levels INFO, WARN, ERROR, and DEBUG. |

---

## 9 — Network and Security

| ID | Requirement |
|----|-------------|
| NET-1 | Pods SHALL have unrestricted outbound internet access. |
| NET-2 | Pods SHALL be network-isolated from other workload pods using Kubernetes NetworkPolicies. |
| NET-3 | Exposed `{project}.localhost` endpoints SHALL be bound to localhost only. |
| NET-4 | Project pods SHALL NOT have access to the Kubernetes API. |
| NET-5 | Project pods SHALL NOT have access to the container runtime or Docker socket. |

---

## 10 — Image Lifecycle

| ID | Requirement |
|----|-------------|
| IMG-1 | The application SHALL automatically remove Docker images that exceed the configured retention period (SET-15). |
| IMG-2 | Image cleanup SHALL run on a recurring timer every hour while the application is running. |
| IMG-3 | When a project image is rebuilt, the previous image for that project SHALL be removed from the local registry. |
| IMG-4 | Container images SHALL be rebuilt only via an explicit user action (Launch or Redeploy). |

---

## 11 — Application Lifecycle

| ID | Requirement |
|----|-------------|
| APP-1 | The application SHALL enforce single-instance execution using a lockfile. |
| APP-2 | If a second instance is launched, the application SHALL display an error message directing the user to close the existing instance. |
| APP-12 | If the lockfile exists but the owning process is no longer running, the application SHALL remove the stale lockfile and proceed with startup. |
| APP-3 | When the application is closed, all deployed pods SHALL continue running in the cluster. |
| APP-4 | On startup, the application SHALL auto-detect previously deployed pods and populate the Pods panel. |
| APP-5 | The application SHALL detect orphaned deployments whose source project directory no longer exists on the host, both on startup and during continuous reconciliation. |
| APP-6 | The application SHALL offer to clean up orphaned deployments by deleting their Helm release, namespace, and generated chart. |
| APP-7 | When the user closes the application during active operations, a shutdown panel SHALL allow immediate cancellation or waiting for tasks to complete. |
| APP-18 | When the user chooses immediate cancellation during shutdown, the application SHALL cancel all in-flight operations (builds, deploys) and terminate all active remote-control sessions. |
| APP-8 | The application SHALL NOT support self-upgrading. |
| APP-9 | The application SHALL NOT provide configuration export or backup functionality. |
| APP-10 | The application SHALL NOT include telemetry or crash reporting. |
| APP-11 | The application SHALL operate in a single-user environment only. |
| APP-13 | The application SHALL maintain a desired-state record of which projects should be deployed and with what configuration. |
| APP-17 | The desired-state record SHALL be stored in a separate state file in the platform app data directory. |
| APP-14 | The application SHALL continuously reconcile actual cluster state toward the desired state. |
| APP-15 | If deployed pods are missing after a cluster recovery, the application SHALL automatically redeploy them to match the desired state. |
| APP-16 | If the config file is malformed or unreadable, the application SHALL fall back to default settings and overwrite the corrupted file. |

---

## 12 — Ingress

| ID | Requirement |
|----|-------------|
| ING-1 | The application SHALL rely on the built-in Traefik ingress controller shipped with k3d/k3s. |
| ING-2 | Exposed services SHALL use hostname-based routing (`{project}.localhost`) sharing port 80 on the host. |

---

## 13 — Error Reporting

| ID | Requirement |
|----|-------------|
| ERR-1 | When an operation fails, the error details SHALL be appended to the active log viewer. |
| ERR-2 | When an operation fails, a toast notification SHALL appear in the bottom-right corner of the window summarizing the error. |
| ERR-3 | Toast notifications SHALL auto-dismiss after 5 seconds. |
| ERR-4 | A maximum of 3 toast notifications SHALL be visible simultaneously; additional toasts SHALL queue until a slot is available. |
| ERR-5 | Clicking a toast notification SHALL navigate to the relevant log viewer entry. |

---

## 14 — UI Quality

| ID | Requirement |
|----|-------------|
| UIQ-1 | No text content SHALL overflow or render outside the bounds of its containing element. |
| UIQ-2 | All icon buttons SHALL display a tooltip describing their action on hover. |
| UIQ-3 | The UI SHALL allow concurrent operations such as builds, deployments, and exposure tasks. |
| UIQ-6 | The application SHALL use per-resource locking to queue conflicting operations that target the same pod or project. |
| UIQ-4 | All panels SHALL be scrollable when their content exceeds the viewport height. |
| UIQ-5 | The application SHALL use a dark theme only. |
