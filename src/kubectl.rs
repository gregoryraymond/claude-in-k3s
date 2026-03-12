use crate::error::{AppResult, CmdResult};
use chrono::{DateTime, Utc};
use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct PodStatus {
    pub name: String,
    pub project: String,
    pub phase: String,
    pub ready: bool,
    pub restart_count: u32,
    pub age: String,
    /// Specific error/warning badges derived from container state and events.
    pub warnings: Vec<String>,
    /// Whether a Service+Ingress exists exposing this pod's project.
    pub exposed: bool,
    /// Container port from the pod spec (0 = unknown).
    pub container_port: u16,
    /// UI selection state (not from k8s).
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub struct NodeStatus {
    #[allow(dead_code)]
    pub name: String,
    pub ready: bool,
    pub version: String,
    pub age: String,
    /// Total allocatable memory in MB.
    pub memory_capacity_mb: Option<u64>,
    /// Memory currently in use in MB (from node conditions, if available).
    pub memory_allocatable_mb: Option<u64>,
}

/// Trait abstracting core Kubernetes operations for pod management, networking,
/// and cluster health checks.
///
/// This trait enables mocking `KubectlRunner` in tests and swapping
/// implementations without changing call-sites.
pub trait KubeOps {
    /// Retrieve all pods in the configured namespace matching the
    /// `app.kubernetes.io/name=claude-code` label selector.
    ///
    /// # Returns
    ///
    /// A vector of `PodStatus` structs describing each pod.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying kubectl command fails to execute.
    fn get_pods(&self) -> impl std::future::Future<Output = AppResult<Vec<PodStatus>>> + Send;

    /// Check whether the Kubernetes cluster is reachable and healthy.
    ///
    /// # Returns
    ///
    /// `true` if `kubectl cluster-info` succeeds, `false` otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if the kubectl process cannot be spawned.
    fn cluster_health(&self) -> impl std::future::Future<Output = AppResult<bool>> + Send;

    /// Delete a pod by name in the configured namespace.
    ///
    /// # Arguments
    ///
    /// * `pod_name` - The name of the pod to delete.
    ///
    /// # Returns
    ///
    /// A `CmdResult` indicating success or failure of the delete operation.
    ///
    /// # Errors
    ///
    /// Returns an error if the kubectl process cannot be spawned.
    fn delete_pod(
        &self,
        pod_name: &str,
    ) -> impl std::future::Future<Output = AppResult<CmdResult>> + Send;

    /// Fetch logs from a pod, optionally falling back to `describe` if the
    /// container has not started.
    ///
    /// # Arguments
    ///
    /// * `pod_name` - The name of the pod.
    /// * `tail_lines` - Maximum number of recent log lines to retrieve.
    ///
    /// # Returns
    ///
    /// A `CmdResult` containing the log output.
    ///
    /// # Errors
    ///
    /// Returns an error if the kubectl process cannot be spawned.
    fn get_logs(
        &self,
        pod_name: &str,
        tail_lines: u32,
    ) -> impl std::future::Future<Output = AppResult<CmdResult>> + Send;

    /// Run `kubectl describe pod` for the given pod name.
    ///
    /// # Arguments
    ///
    /// * `pod_name` - The name of the pod to describe.
    ///
    /// # Returns
    ///
    /// A `CmdResult` containing the describe output.
    ///
    /// # Errors
    ///
    /// Returns an error if the kubectl process cannot be spawned.
    fn describe_pod(
        &self,
        pod_name: &str,
    ) -> impl std::future::Future<Output = AppResult<CmdResult>> + Send;

    /// Create a ClusterIP `Service` exposing a project on the given port.
    ///
    /// # Arguments
    ///
    /// * `project` - The project label value used for the service selector.
    /// * `port` - The port to expose.
    ///
    /// # Returns
    ///
    /// A `CmdResult` indicating success or failure.
    ///
    /// # Errors
    ///
    /// Returns an error if the kubectl process cannot be spawned.
    fn create_service(
        &self,
        project: &str,
        port: u16,
    ) -> impl std::future::Future<Output = AppResult<CmdResult>> + Send;

    /// Create an `Ingress` routing `{project}.localhost` to the project service.
    ///
    /// # Arguments
    ///
    /// * `project` - The project label value.
    /// * `port` - The backend port number.
    ///
    /// # Returns
    ///
    /// A `CmdResult` indicating success or failure.
    ///
    /// # Errors
    ///
    /// Returns an error if the kubectl process cannot be spawned.
    fn create_ingress(
        &self,
        project: &str,
        port: u16,
    ) -> impl std::future::Future<Output = AppResult<CmdResult>> + Send;

    /// Detect the first listening TCP port inside a running pod.
    ///
    /// # Arguments
    ///
    /// * `pod_name` - The name of the pod to inspect.
    ///
    /// # Returns
    ///
    /// A tuple `(port, detected)` where `detected` is `true` when a real
    /// listening port was found, and `false` when falling back to 8080.
    fn detect_listening_port(
        &self,
        pod_name: &str,
    ) -> impl std::future::Future<Output = (u16, bool)> + Send;

    /// Create or update a Kubernetes `Secret` from environment variable pairs.
    ///
    /// # Arguments
    ///
    /// * `project` - The project name used to derive the secret name.
    /// * `env_vars` - Slice of `(key, value)` pairs for the secret data.
    ///
    /// # Returns
    ///
    /// A `CmdResult` indicating success or failure.
    ///
    /// # Errors
    ///
    /// Returns an error if the kubectl process cannot be spawned.
    fn apply_secret_from_env(
        &self,
        project: &str,
        env_vars: &[(String, String)],
    ) -> impl std::future::Future<Output = AppResult<CmdResult>> + Send;

    /// Enrich a slice of pods with warning messages sourced from recent
    /// Kubernetes events in the namespace.
    ///
    /// # Arguments
    ///
    /// * `pods` - Mutable slice of `PodStatus` to enrich in-place.
    ///
    /// # Errors
    ///
    /// Returns an error if the kubectl process cannot be spawned.
    fn enrich_pods_with_events(
        &self,
        pods: &mut [PodStatus],
    ) -> impl std::future::Future<Output = AppResult<()>> + Send;
}

pub struct KubectlRunner {
    binary: String,
    namespace: String,
}

/// Parse Kubernetes memory strings like "8146280Ki", "4Gi", "2048Mi" into MB.
fn parse_k8s_memory(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.ends_with("Ki") {
        s.trim_end_matches("Ki").parse::<u64>().ok().map(|v| v / 1024)
    } else if s.ends_with("Mi") {
        s.trim_end_matches("Mi").parse::<u64>().ok()
    } else if s.ends_with("Gi") {
        s.trim_end_matches("Gi").parse::<u64>().ok().map(|v| v * 1024)
    } else if s.ends_with("Ti") {
        s.trim_end_matches("Ti").parse::<u64>().ok().map(|v| v * 1024 * 1024)
    } else {
        // Plain bytes
        s.parse::<u64>().ok().map(|v| v / (1024 * 1024))
    }
}

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

impl KubectlRunner {
    pub fn new(binary: &str, namespace: &str) -> Self {
        Self {
            binary: binary.into(),
            namespace: namespace.into(),
        }
    }

    async fn run(&self, args: &[&str]) -> AppResult<CmdResult> {
        let output = Command::new(&self.binary)
            .args(args)
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

    pub async fn get_pods(&self) -> AppResult<Vec<PodStatus>> {
        let result = self
            .run(&[
                "get",
                "pods",
                "-n",
                &self.namespace,
                "-l",
                "app.kubernetes.io/name=claude-code",
                "-o",
                "json",
            ])
            .await?;

        if !result.success {
            return Ok(vec![]);
        }

        let pod_list: serde_json::Value = serde_json::from_str(&result.stdout)?;
        let mut pods = Vec::new();

        if let Some(items) = pod_list["items"].as_array() {
            for item in items {
                let name = item["metadata"]["name"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string();
                let project = item["metadata"]["labels"]["claude-code/project"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string();
                let phase = item["status"]["phase"]
                    .as_str()
                    .unwrap_or("Unknown")
                    .to_string();

                let container_statuses = item["status"]["containerStatuses"].as_array();
                let (ready, restart_count) = container_statuses
                    .and_then(|cs| cs.first())
                    .map(|cs| {
                        let ready = cs["ready"].as_bool().unwrap_or(false);
                        let restarts = cs["restartCount"].as_u64().unwrap_or(0) as u32;
                        (ready, restarts)
                    })
                    .unwrap_or((false, 0));

                // Use container waiting reason as phase if available (more descriptive)
                let display_phase = container_statuses
                    .and_then(|cs| cs.first())
                    .and_then(|cs| {
                        cs["state"]["waiting"]["reason"]
                            .as_str()
                            .map(|s| s.to_string())
                    })
                    .unwrap_or(phase);

                // Collect warnings from container state, terminated state, and conditions
                let mut warnings = Vec::new();

                if let Some(cs) = container_statuses.and_then(|cs| cs.first()) {
                    // Waiting message (e.g. "Back-off pulling image...")
                    if let Some(msg) = cs["state"]["waiting"]["message"].as_str() {
                        let short = truncate_warning(msg);
                        if !short.is_empty() {
                            warnings.push(short);
                        }
                    }
                    // Terminated reason (e.g. OOMKilled, Error)
                    if let Some(reason) = cs["state"]["terminated"]["reason"].as_str() {
                        if reason != "Completed" {
                            warnings.push(reason.to_string());
                        }
                    }
                    // Previous termination (last crash reason)
                    if let Some(reason) = cs["lastState"]["terminated"]["reason"].as_str() {
                        if reason != "Completed" {
                            warnings.push(format!("Last: {}", reason));
                        }
                    }
                    if let Some(exit_code) = cs["lastState"]["terminated"]["exitCode"].as_i64() {
                        if exit_code != 0 {
                            warnings.push(format!("Exit: {}", exit_code));
                        }
                    }
                }

                // Pod conditions (scheduling, init failures)
                if let Some(conditions) = item["status"]["conditions"].as_array() {
                    for cond in conditions {
                        let status = cond["status"].as_str().unwrap_or("");
                        let ctype = cond["type"].as_str().unwrap_or("");
                        if status == "False" {
                            if let Some(reason) = cond["reason"].as_str() {
                                match reason {
                                    "Unschedulable" => warnings.push("Unschedulable".into()),
                                    _ if ctype == "PodScheduled" => {
                                        warnings.push(format!("Scheduling: {}", reason));
                                    }
                                    _ if ctype == "Initialized" => {
                                        warnings.push(format!("Init: {}", reason));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }

                let container_port = item["spec"]["containers"]
                    .as_array()
                    .and_then(|cs| cs.first())
                    .and_then(|c| c["ports"].as_array())
                    .and_then(|ps| ps.first())
                    .and_then(|p| p["containerPort"].as_u64())
                    .unwrap_or(0) as u16;

                let age = format_age(
                    item["metadata"]["creationTimestamp"]
                        .as_str()
                        .unwrap_or(""),
                );

                pods.push(PodStatus {
                    name,
                    project,
                    phase: display_phase,
                    ready,
                    restart_count,
                    age,
                    warnings,
                    exposed: false,
                    container_port,
                    selected: false,
                });
            }
        }

        Ok(pods)
    }



    pub async fn get_logs(&self, pod_name: &str, tail_lines: u32) -> AppResult<CmdResult> {
        tracing::debug!("Fetching logs for pod '{}' (tail={})", pod_name, tail_lines);
        let tail = tail_lines.to_string();
        let result = self
            .run(&["logs", "-n", &self.namespace, pod_name, "--tail", &tail])
            .await?;

        // If logs failed (e.g. container not started), fall back to describe
        if !result.success {
            return self.describe_pod(pod_name).await;
        }

        // If the pod has restarted, also fetch previous container logs
        let prev = self
            .run(&["logs", "-n", &self.namespace, pod_name, "--previous", "--tail", &tail])
            .await;
        if let Ok(prev_result) = prev {
            if prev_result.success && !prev_result.stdout.trim().is_empty() {
                let mut combined = String::new();
                combined.push_str("=== Previous container logs (crash reason) ===\n");
                combined.push_str(&prev_result.stdout);
                combined.push_str("\n=== Current container logs ===\n");
                combined.push_str(&result.stdout);
                return Ok(CmdResult {
                    success: true,
                    stdout: combined,
                    stderr: result.stderr,
                });
            }
        }

        Ok(result)
    }

    pub async fn describe_pod(&self, pod_name: &str) -> AppResult<CmdResult> {
        self.run(&["describe", "pod", "-n", &self.namespace, pod_name])
            .await
    }

    pub async fn cluster_health(&self) -> AppResult<bool> {
        let result = self.run(&["cluster-info"]).await?;
        Ok(result.success)
    }

    pub async fn get_nodes(&self) -> AppResult<Vec<NodeStatus>> {
        let result = self.run(&["get", "nodes", "-o", "json"]).await?;

        if !result.success {
            return Ok(vec![]);
        }

        let node_list: serde_json::Value = serde_json::from_str(&result.stdout)?;
        let mut nodes = Vec::new();

        if let Some(items) = node_list["items"].as_array() {
            for item in items {
                let name = item["metadata"]["name"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string();
                let ready = item["status"]["conditions"]
                    .as_array()
                    .and_then(|conds| {
                        conds.iter().find(|c| c["type"].as_str() == Some("Ready"))
                    })
                    .and_then(|c| c["status"].as_str())
                    .map(|s| s == "True")
                    .unwrap_or(false);
                let version = item["status"]["nodeInfo"]["kubeletVersion"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let age = item["metadata"]["creationTimestamp"]
                    .as_str()
                    .map(format_age)
                    .unwrap_or_default();
                let memory_capacity_mb = item["status"]["capacity"]["memory"]
                    .as_str()
                    .and_then(parse_k8s_memory);
                let memory_allocatable_mb = item["status"]["allocatable"]["memory"]
                    .as_str()
                    .and_then(parse_k8s_memory);
                nodes.push(NodeStatus { name, ready, version, age, memory_capacity_mb, memory_allocatable_mb });
            }
        }

        Ok(nodes)
    }

    /// Get actual memory usage from `kubectl top node`.
    /// Returns (used_mb, capacity_mb) if metrics-server is available.
    pub async fn top_node_memory(&self) -> Option<(u64, u64)> {
        let result = self.run(&["top", "node", "--no-headers"]).await.ok()?;
        if !result.success {
            return None;
        }
        // Output format: "NODE  CPU(cores)  CPU%  MEMORY(bytes)  MEMORY%"
        // e.g.: "k3d-claude-server-0   102m   5%    1842Mi   23%"
        for line in result.stdout.lines() {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() >= 5 {
                let mem_used = parse_k8s_memory(cols[3]);
                let mem_pct: Option<u64> = cols[4].trim_end_matches('%').parse().ok();
                if let (Some(used), Some(pct)) = (mem_used, mem_pct) {
                    if pct > 0 {
                        let total = used * 100 / pct;
                        return Some((used, total));
                    }
                }
            }
        }
        None
    }

    pub async fn delete_pod(&self, pod_name: &str) -> AppResult<CmdResult> {
        tracing::info!("Deleting pod '{}'", pod_name);
        let result = self.run(&["delete", "pod", "-n", &self.namespace, pod_name])
            .await;
        match &result {
            Ok(r) if r.success => tracing::debug!("Pod '{}' deleted successfully", pod_name),
            Ok(r) => tracing::warn!("Pod '{}' delete failed: {}", pod_name, r.stderr.trim()),
            Err(e) => tracing::warn!("Pod '{}' delete error: {}", pod_name, e),
        }
        result
    }

    /// Detect the first listening TCP port inside a pod.
    /// Returns `(port, detected)` where `detected` is `true` when a real
    /// listening port was found and `false` when falling back to the default 8080.
    pub async fn detect_listening_port(&self, pod_name: &str) -> (u16, bool) {
        tracing::debug!("Detecting listening port in pod '{}'", pod_name);
        let result = self
            .run(&[
                "exec",
                "-n",
                &self.namespace,
                pod_name,
                "--",
                "sh",
                "-c",
                "ss -tlnp 2>/dev/null || netstat -tlnp 2>/dev/null",
            ])
            .await;

        if let Ok(r) = result {
            if r.success {
                // Parse lines like "LISTEN 0 128 *:8080 *:*" or "tcp 0 0 0.0.0.0:3000 ..."
                for line in r.stdout.lines().skip(1) {
                    // Look for :PORT pattern
                    for part in line.split_whitespace() {
                        if let Some(colon_pos) = part.rfind(':') {
                            if let Ok(port) = part[colon_pos + 1..].parse::<u16>() {
                                if port > 0 {
                                    tracing::debug!("Detected listening port {} in pod '{}'", port, pod_name);
                                    return (port, true);
                                }
                            }
                        }
                    }
                }
            }
        }

        tracing::debug!("No listening port detected in pod '{}', using default 8080", pod_name);
        (8080, false)
    }

    /// POD-47: Detect all listening TCP ports inside a pod.
    /// Returns a Vec of ports (may be empty if none found).
    pub async fn detect_all_listening_ports(&self, pod_name: &str) -> Vec<u16> {
        let result = self
            .run(&[
                "exec",
                "-n",
                &self.namespace,
                pod_name,
                "--",
                "sh",
                "-c",
                "ss -tlnp 2>/dev/null || netstat -tlnp 2>/dev/null",
            ])
            .await;

        let mut ports = Vec::new();
        if let Ok(r) = result {
            if r.success {
                for line in r.stdout.lines().skip(1) {
                    for part in line.split_whitespace() {
                        if let Some(colon_pos) = part.rfind(':') {
                            if let Ok(port) = part[colon_pos + 1..].parse::<u16>() {
                                if port > 0 && !ports.contains(&port) {
                                    ports.push(port);
                                }
                            }
                        }
                    }
                }
            }
        }
        ports
    }

    /// Create a ClusterIP Service for a project.
    pub async fn create_service(&self, project: &str, port: u16) -> AppResult<CmdResult> {
        tracing::info!("Creating service for project '{}' on port {}", project, port);
        let svc_name = format!("svc-{}", project);
        let yaml = format!(
            r#"apiVersion: v1
kind: Service
metadata:
  name: {svc_name}
  namespace: {ns}
  labels:
    claude-code/project: {project}
spec:
  selector:
    claude-code/project: {project}
  ports:
    - port: {port}
      targetPort: {port}
      protocol: TCP
"#,
            svc_name = svc_name,
            ns = self.namespace,
            project = project,
            port = port,
        );

        let result = Command::new(&self.binary)
            .args(["apply", "-f", "-", "-n", &self.namespace])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let res = match result {
            Ok(mut child) => {
                if let Some(stdin) = child.stdin.as_mut() {
                    use tokio::io::AsyncWriteExt;
                    let _ = stdin.write_all(yaml.as_bytes()).await;
                    let _ = stdin.shutdown().await;
                }
                let output = child.wait_with_output().await?;
                Ok(CmdResult {
                    success: output.status.success(),
                    stdout: String::from_utf8_lossy(&output.stdout).into(),
                    stderr: String::from_utf8_lossy(&output.stderr).into(),
                })
            }
            Err(e) => Err(e.into()),
        };
        match &res {
            Ok(r) if r.success => tracing::debug!("Service created for project '{}'", project),
            Ok(r) => tracing::warn!("Service creation failed for project '{}': {}", project, r.stderr.trim()),
            Err(e) => tracing::warn!("Service creation error for project '{}': {}", project, e),
        }
        res
    }

    /// POD-47: Create a ClusterIP Service exposing multiple ports.
    pub async fn create_service_multi(&self, project: &str, ports: &[u16]) -> AppResult<CmdResult> {
        if ports.is_empty() {
            return self.create_service(project, 8080).await;
        }
        if ports.len() == 1 {
            return self.create_service(project, ports[0]).await;
        }
        tracing::info!("Creating multi-port service for project '{}' on ports {:?}", project, ports);
        let svc_name = format!("svc-{}", project);
        let ports_yaml: String = ports.iter().enumerate().map(|(i, port)| {
            format!(
                "    - name: port-{i}\n      port: {port}\n      targetPort: {port}\n      protocol: TCP\n",
                i = i, port = port,
            )
        }).collect();
        let yaml = format!(
            r#"apiVersion: v1
kind: Service
metadata:
  name: {svc_name}
  namespace: {ns}
  labels:
    claude-code/project: {project}
spec:
  selector:
    claude-code/project: {project}
  ports:
{ports_yaml}"#,
            svc_name = svc_name,
            ns = self.namespace,
            project = project,
            ports_yaml = ports_yaml,
        );

        let result = Command::new(&self.binary)
            .args(["apply", "-f", "-", "-n", &self.namespace])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        match result {
            Ok(mut child) => {
                if let Some(stdin) = child.stdin.as_mut() {
                    use tokio::io::AsyncWriteExt;
                    let _ = stdin.write_all(yaml.as_bytes()).await;
                    let _ = stdin.shutdown().await;
                }
                let output = child.wait_with_output().await?;
                Ok(CmdResult {
                    success: output.status.success(),
                    stdout: String::from_utf8_lossy(&output.stdout).into(),
                    stderr: String::from_utf8_lossy(&output.stderr).into(),
                })
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Create an Ingress routing `{project}.localhost` to the service.
    pub async fn create_ingress(&self, project: &str, port: u16) -> AppResult<CmdResult> {
        tracing::info!("Creating ingress for project '{}' on port {}", project, port);
        let ingress_name = format!("ingress-{}", project);
        let svc_name = format!("svc-{}", project);
        let host = format!("{}.localhost", project);
        let yaml = format!(
            r#"apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: {ingress_name}
  namespace: {ns}
  labels:
    claude-code/project: {project}
spec:
  rules:
    - host: {host}
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: {svc_name}
                port:
                  number: {port}
"#,
            ingress_name = ingress_name,
            ns = self.namespace,
            project = project,
            host = host,
            svc_name = svc_name,
            port = port,
        );

        let result = Command::new(&self.binary)
            .args(["apply", "-f", "-", "-n", &self.namespace])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let res = match result {
            Ok(mut child) => {
                if let Some(stdin) = child.stdin.as_mut() {
                    use tokio::io::AsyncWriteExt;
                    let _ = stdin.write_all(yaml.as_bytes()).await;
                    let _ = stdin.shutdown().await;
                }
                let output = child.wait_with_output().await?;
                Ok(CmdResult {
                    success: output.status.success(),
                    stdout: String::from_utf8_lossy(&output.stdout).into(),
                    stderr: String::from_utf8_lossy(&output.stderr).into(),
                })
            }
            Err(e) => Err(e.into()),
        };
        match &res {
            Ok(r) if r.success => tracing::debug!("Ingress created for project '{}'", project),
            Ok(r) => tracing::warn!("Ingress creation failed for project '{}': {}", project, r.stderr.trim()),
            Err(e) => tracing::warn!("Ingress creation error for project '{}': {}", project, e),
        }
        res
    }

    /// Delete the Service for a project.
    pub async fn delete_service(&self, project: &str) -> AppResult<CmdResult> {
        tracing::info!("Deleting service for project '{}'", project);
        let svc_name = format!("svc-{}", project);
        let result = self.run(&["delete", "svc", &svc_name, "-n", &self.namespace, "--ignore-not-found"])
            .await;
        match &result {
            Ok(r) if r.success => tracing::debug!("Service deleted for project '{}'", project),
            Ok(r) => tracing::warn!("Service delete failed for project '{}': {}", project, r.stderr.trim()),
            Err(e) => tracing::warn!("Service delete error for project '{}': {}", project, e),
        }
        result
    }

    /// Delete the Ingress for a project.
    pub async fn delete_ingress(&self, project: &str) -> AppResult<CmdResult> {
        tracing::info!("Deleting ingress for project '{}'", project);
        let ingress_name = format!("ingress-{}", project);
        let result = self.run(&["delete", "ingress", &ingress_name, "-n", &self.namespace, "--ignore-not-found"])
            .await;
        match &result {
            Ok(r) if r.success => tracing::debug!("Ingress deleted for project '{}'", project),
            Ok(r) => tracing::warn!("Ingress delete failed for project '{}': {}", project, r.stderr.trim()),
            Err(e) => tracing::warn!("Ingress delete error for project '{}': {}", project, e),
        }
        result
    }

    /// POD-57: Delete a namespace (used for cleanup after helm uninstall).
    pub async fn delete_namespace(&self, namespace: &str) -> AppResult<CmdResult> {
        tracing::info!("Deleting namespace '{}'", namespace);
        self.run(&["delete", "namespace", namespace, "--ignore-not-found"]).await
    }

    /// PRJ-51: Create or update a Secret from env vars using kubectl apply.
    pub async fn apply_secret_from_env(
        &self,
        project: &str,
        env_vars: &[(String, String)],
    ) -> AppResult<CmdResult> {
        let secret_name = format!("env-{}", project);
        let mut args = vec![
            "create".to_string(),
            "secret".to_string(),
            "generic".to_string(),
            secret_name.clone(),
            "-n".to_string(),
            self.namespace.clone(),
            "--dry-run=client".to_string(),
            "-o".to_string(),
            "yaml".to_string(),
        ];
        for (k, v) in env_vars {
            args.push(format!("--from-literal={}={}", k, v));
        }

        // Step 1: Generate YAML
        let output = Command::new(&self.binary)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            return Ok(CmdResult {
                success: false,
                stdout: String::from_utf8_lossy(&output.stdout).into(),
                stderr: String::from_utf8_lossy(&output.stderr).into(),
            });
        }

        // Step 2: Apply the YAML
        let yaml = output.stdout;
        let mut child = Command::new(&self.binary)
            .args(["apply", "-f", "-", "-n", &self.namespace])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(stdin) = child.stdin.as_mut() {
            use tokio::io::AsyncWriteExt;
            let _ = stdin.write_all(&yaml).await;
        }

        let result = child.wait_with_output().await?;
        Ok(CmdResult {
            success: result.status.success(),
            stdout: String::from_utf8_lossy(&result.stdout).into(),
            stderr: String::from_utf8_lossy(&result.stderr).into(),
        })
    }

    /// Delete a secret for a project.
    pub async fn delete_secret(&self, project: &str) -> AppResult<CmdResult> {
        let secret_name = format!("env-{}", project);
        self.run(&["delete", "secret", &secret_name, "-n", &self.namespace, "--ignore-not-found"])
            .await
    }

    /// Get list of project names that have Services in the namespace.
    pub async fn get_services(&self) -> AppResult<Vec<String>> {
        let result = self
            .run(&[
                "get",
                "svc",
                "-n",
                &self.namespace,
                "-l",
                "claude-code/project",
                "-o",
                "json",
            ])
            .await?;

        if !result.success {
            return Ok(vec![]);
        }

        let svc_list: serde_json::Value = serde_json::from_str(&result.stdout)?;
        let mut projects = Vec::new();

        if let Some(items) = svc_list["items"].as_array() {
            for item in items {
                if let Some(project) = item["metadata"]["labels"]["claude-code/project"].as_str() {
                    projects.push(project.to_string());
                }
            }
        }

        Ok(projects)
    }

    /// Fetch recent events in the namespace and enrich pods with event-sourced warnings.
    pub async fn enrich_pods_with_events(&self, pods: &mut [PodStatus]) -> AppResult<()> {
        let result = self
            .run(&["get", "events", "-n", &self.namespace, "-o", "json"])
            .await?;

        if !result.success {
            return Ok(());
        }

        let events: serde_json::Value = match serde_json::from_str(&result.stdout) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        let items = match events["items"].as_array() {
            Some(items) => items,
            None => return Ok(()),
        };

        for event in items {
            let event_type = event["type"].as_str().unwrap_or("");
            if event_type != "Warning" {
                continue;
            }

            let involved_name = event["involvedObject"]["name"].as_str().unwrap_or("");
            let reason = event["reason"].as_str().unwrap_or("");
            let message = event["message"].as_str().unwrap_or("");

            // Match event to a pod
            for pod in pods.iter_mut() {
                if involved_name != pod.name {
                    continue;
                }

                let warning = match reason {
                    "FailedMount" | "FailedAttachVolume" => {
                        Some(format!("FailedMount: {}", truncate_warning(message)))
                    }
                    "FailedScheduling" => {
                        Some(format!("FailedScheduling: {}", truncate_warning(message)))
                    }
                    "FailedCreate" => {
                        Some(format!("FailedCreate: {}", truncate_warning(message)))
                    }
                    "BackOff" => Some("CrashBackOff".into()),
                    "Unhealthy" => {
                        Some(format!("Unhealthy: {}", truncate_warning(message)))
                    }
                    "InsufficientCPU" | "InsufficientMemory" => {
                        Some(reason.to_string())
                    }
                    "FailedCreatePodSandBox" => {
                        Some(format!("SandboxFailed: {}", truncate_warning(message)))
                    }
                    _ => None,
                };

                if let Some(w) = warning {
                    if !pod.warnings.contains(&w) {
                        pod.warnings.push(w);
                    }
                }
            }
        }

        // Clear event-sourced warnings for pods that have recovered (Running + Ready)
        // but keep last-termination warnings (Last:, Exit:) so restart reasons are visible
        for pod in pods.iter_mut() {
            if pod.phase == "Running" && pod.ready && pod.restart_count == 0 {
                pod.warnings.clear();
            } else if pod.phase == "Running" && pod.ready {
                // Keep only last-termination warnings for restarted pods
                pod.warnings.retain(|w| w.starts_with("Last:") || w.starts_with("Exit:"));
            }
        }

        Ok(())
    }
}

impl KubeOps for KubectlRunner {
    async fn get_pods(&self) -> AppResult<Vec<PodStatus>> {
        KubectlRunner::get_pods(self).await
    }

    async fn cluster_health(&self) -> AppResult<bool> {
        KubectlRunner::cluster_health(self).await
    }

    async fn delete_pod(&self, pod_name: &str) -> AppResult<CmdResult> {
        KubectlRunner::delete_pod(self, pod_name).await
    }

    async fn get_logs(&self, pod_name: &str, tail_lines: u32) -> AppResult<CmdResult> {
        KubectlRunner::get_logs(self, pod_name, tail_lines).await
    }

    async fn describe_pod(&self, pod_name: &str) -> AppResult<CmdResult> {
        KubectlRunner::describe_pod(self, pod_name).await
    }

    async fn create_service(&self, project: &str, port: u16) -> AppResult<CmdResult> {
        KubectlRunner::create_service(self, project, port).await
    }

    async fn create_ingress(&self, project: &str, port: u16) -> AppResult<CmdResult> {
        KubectlRunner::create_ingress(self, project, port).await
    }

    async fn detect_listening_port(&self, pod_name: &str) -> (u16, bool) {
        KubectlRunner::detect_listening_port(self, pod_name).await
    }

    async fn apply_secret_from_env(
        &self,
        project: &str,
        env_vars: &[(String, String)],
    ) -> AppResult<CmdResult> {
        KubectlRunner::apply_secret_from_env(self, project, env_vars).await
    }

    async fn enrich_pods_with_events(&self, pods: &mut [PodStatus]) -> AppResult<()> {
        KubectlRunner::enrich_pods_with_events(self, pods).await
    }
}

/// Truncate a warning message to a reasonable badge length.
fn truncate_warning(msg: &str) -> String {
    // Take first meaningful clause (up to first comma, semicolon, or 80 chars)
    let trimmed = msg.trim();
    let end = trimmed
        .find(';')
        .or_else(|| {
            // Allow one comma (for messages like "path X is not a directory"),
            // truncate at the second comma
            trimmed
                .find(',')
                .and_then(|first| trimmed[first + 1..].find(',').map(|second| first + 1 + second))
        })
        .unwrap_or(trimmed.len())
        .min(80);
    trimmed[..end].to_string()
}

/// Extract the Events section and Last State from `kubectl describe pod` output.
///
/// # Arguments
///
/// * `describe_output` - The raw text output from `kubectl describe pod`
///
/// # Returns
///
/// `Some(String)` containing the extracted Last State and/or Events sections,
/// or `None` if neither section is found.
pub fn extract_describe_events(describe_output: &str) -> Option<String> {
    let mut result = String::new();

    // Extract Last State (shows termination reason with detail)
    let mut in_last_state = false;
    let mut last_state_indent = 0;
    for line in describe_output.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        if trimmed.starts_with("Last State:") {
            in_last_state = true;
            last_state_indent = indent;
            result.push_str(trimmed);
            result.push('\n');
            continue;
        }
        if in_last_state {
            if indent > last_state_indent && !trimmed.is_empty() {
                result.push_str("  ");
                result.push_str(trimmed);
                result.push('\n');
            } else {
                in_last_state = false;
            }
        }
    }

    // Extract Events section (at the end of describe output)
    if let Some(events_pos) = describe_output.find("\nEvents:") {
        let events = &describe_output[events_pos + 1..];
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(events.trim_end());
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_mock_script(dir: &TempDir, content: &str) -> String {
        let script_path = dir.path().join("kubectl");
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(0o755)
                .open(&script_path)
                .unwrap();
            f.write_all(content.as_bytes()).unwrap();
            f.sync_all().unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        script_path.to_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn get_pods_parses_json() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            r#"#!/bin/bash
cat << 'ENDJSON'
{
  "items": [
    {
      "metadata": {
        "name": "claude-pod-1",
        "labels": {
          "claude-code/project": "my-project"
        },
        "creationTimestamp": "2025-01-15T10:30:00Z"
      },
      "status": {
        "phase": "Running",
        "containerStatuses": [
          {
            "ready": true,
            "restartCount": 2
          }
        ]
      }
    },
    {
      "metadata": {
        "name": "claude-pod-2",
        "labels": {
          "claude-code/project": "other-project"
        },
        "creationTimestamp": "2025-01-16T08:00:00Z"
      },
      "status": {
        "phase": "Pending",
        "containerStatuses": [
          {
            "ready": false,
            "restartCount": 0
          }
        ]
      }
    }
  ]
}
ENDJSON
"#,
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let pods = runner.get_pods().await.unwrap();

        assert_eq!(pods.len(), 2);

        assert_eq!(pods[0].name, "claude-pod-1");
        assert_eq!(pods[0].project, "my-project");
        assert_eq!(pods[0].phase, "Running");
        assert!(pods[0].ready);
        assert_eq!(pods[0].restart_count, 2);
        assert!(!pods[0].age.is_empty() && pods[0].age != "2025-01-15T10:30:00Z");

        assert_eq!(pods[1].name, "claude-pod-2");
        assert_eq!(pods[1].project, "other-project");
        assert_eq!(pods[1].phase, "Pending");
        assert!(!pods[1].ready);
        assert_eq!(pods[1].restart_count, 0);
        assert!(!pods[1].age.is_empty() && pods[1].age != "2025-01-16T08:00:00Z");
    }

    #[tokio::test]
    async fn get_pods_empty_items() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            r#"#!/bin/bash
cat << 'ENDJSON'
{"items": []}
ENDJSON
"#,
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let pods = runner.get_pods().await.unwrap();
        assert!(pods.is_empty());
    }

    #[tokio::test]
    async fn get_pods_command_failure_returns_empty() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            "#!/bin/bash\nexit 1\n",
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let pods = runner.get_pods().await.unwrap();
        assert!(pods.is_empty());
    }

    #[tokio::test]
    async fn get_pods_missing_labels_uses_defaults() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            r#"#!/bin/bash
cat << 'ENDJSON'
{
  "items": [
    {
      "metadata": {
        "name": "bare-pod"
      },
      "status": {
        "phase": "Running"
      }
    }
  ]
}
ENDJSON
"#,
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let pods = runner.get_pods().await.unwrap();

        assert_eq!(pods.len(), 1);
        assert_eq!(pods[0].name, "bare-pod");
        assert_eq!(pods[0].project, "unknown");
        assert_eq!(pods[0].phase, "Running");
        assert!(!pods[0].ready);
        assert_eq!(pods[0].restart_count, 0);
        assert_eq!(pods[0].age, "unknown");
    }

    #[tokio::test]
    async fn get_pods_missing_container_statuses() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            r#"#!/bin/bash
cat << 'ENDJSON'
{
  "items": [
    {
      "metadata": {
        "name": "no-containers-pod",
        "labels": {
          "claude-code/project": "proj-x"
        },
        "creationTimestamp": "2025-02-01T12:00:00Z"
      },
      "status": {
        "phase": "Pending"
      }
    }
  ]
}
ENDJSON
"#,
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let pods = runner.get_pods().await.unwrap();

        assert_eq!(pods.len(), 1);
        assert_eq!(pods[0].name, "no-containers-pod");
        assert_eq!(pods[0].project, "proj-x");
        assert_eq!(pods[0].phase, "Pending");
        assert!(!pods[0].ready);
        assert_eq!(pods[0].restart_count, 0);
        assert!(!pods[0].age.is_empty() && pods[0].age != "2025-02-01T12:00:00Z");
    }

    #[tokio::test]
    async fn cluster_health_healthy() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            "#!/bin/bash\nexit 0\n",
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let healthy = runner.cluster_health().await.unwrap();
        assert!(healthy);
    }

    #[tokio::test]
    async fn cluster_health_unhealthy() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            "#!/bin/bash\nexit 1\n",
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let healthy = runner.cluster_health().await.unwrap();
        assert!(!healthy);
    }

    #[tokio::test]
    async fn delete_pod_success() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            "#!/bin/bash\nexit 0\n",
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let result = runner.delete_pod("test-pod").await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn delete_pod_verifies_args() {
        let dir = TempDir::new().unwrap();
        let args_file = dir.path().join("recorded_args");
        let args_file_str = args_file.to_str().unwrap();
        let script = format!(
            "#!/bin/bash\nprintf '%s ' \"$@\" > \"{}\"\nexit 0\n",
            args_file_str
        );
        let binary = create_mock_script(&dir, &script);

        let runner = KubectlRunner::new(&binary, "claude-code");
        runner.delete_pod("test-pod-123").await.unwrap();

        let recorded = fs::read_to_string(&args_file).unwrap();
        assert_eq!(recorded.trim(), "delete pod -n claude-code test-pod-123");
    }

    #[tokio::test]
    async fn get_logs_success() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            "#!/bin/bash\nprintf 'line1\\nline2\\n'\nexit 0\n",
        );

        let runner = KubectlRunner::new(&binary, "test-ns");
        let result = runner.get_logs("my-pod", 50).await.unwrap();
        assert!(result.success);
        assert_eq!(result.stdout, "line1\nline2\n");
    }

    #[tokio::test]
    async fn get_logs_verifies_args() {
        let dir = TempDir::new().unwrap();
        let args_file = dir.path().join("recorded_args");
        let args_file_str = args_file.to_str().unwrap();
        let script = format!(
            "#!/bin/bash\nprintf '%s ' \"$@\" > \"{}\"\nexit 0\n",
            args_file_str
        );
        let binary = create_mock_script(&dir, &script);

        let runner = KubectlRunner::new(&binary, "test-ns");
        runner.get_logs("my-pod", 100).await.unwrap();

        let recorded = fs::read_to_string(&args_file).unwrap();
        assert_eq!(recorded.trim(), "logs -n test-ns my-pod --tail 100");
    }

    #[test]
    fn pod_status_clone() {
        let pod = PodStatus {
            name: "test-pod".to_string(),
            project: "test-project".to_string(),
            phase: "Running".to_string(),
            ready: true,
            restart_count: 3,
            age: "2025-01-01T00:00:00Z".to_string(),
            warnings: vec!["OOMKilled".to_string()],
            exposed: false,
            container_port: 8080,
            selected: false,
        };

        let cloned = pod.clone();

        assert_eq!(pod.name, cloned.name);
        assert_eq!(pod.project, cloned.project);
        assert_eq!(pod.phase, cloned.phase);
        assert_eq!(pod.ready, cloned.ready);
        assert_eq!(pod.restart_count, cloned.restart_count);
        assert_eq!(pod.age, cloned.age);
        assert_eq!(pod.warnings, cloned.warnings);
        assert_eq!(pod.exposed, cloned.exposed);
        assert_eq!(pod.container_port, cloned.container_port);
        assert_eq!(pod.selected, cloned.selected);
    }

    #[test]
    fn format_age_days_and_hours() {
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

    #[tokio::test]
    async fn create_service_applies_yaml() {
        let dir = TempDir::new().unwrap();
        let stdin_file = dir.path().join("captured_stdin");
        let stdin_file_str = stdin_file.to_str().unwrap();
        let script = format!(
            "#!/bin/bash\ncat > \"{}\"\nexit 0\n",
            stdin_file_str
        );
        let binary = create_mock_script(&dir, &script);

        let runner = KubectlRunner::new(&binary, "claude-code");
        let result = runner.create_service("my-app", 3000).await.unwrap();
        assert!(result.success);

        let yaml = fs::read_to_string(&stdin_file).unwrap();
        assert!(yaml.contains("name: svc-my-app"), "expected svc name in yaml: {}", yaml);
        assert!(yaml.contains("namespace: claude-code"), "expected namespace in yaml: {}", yaml);
        assert!(yaml.contains("claude-code/project: my-app"), "expected project label in yaml: {}", yaml);
        assert!(yaml.contains("port: 3000"), "expected port in yaml: {}", yaml);
        assert!(yaml.contains("targetPort: 3000"), "expected targetPort in yaml: {}", yaml);
    }

    #[tokio::test]
    async fn create_ingress_applies_yaml() {
        let dir = TempDir::new().unwrap();
        let stdin_file = dir.path().join("captured_stdin");
        let stdin_file_str = stdin_file.to_str().unwrap();
        let script = format!(
            "#!/bin/bash\ncat > \"{}\"\nexit 0\n",
            stdin_file_str
        );
        let binary = create_mock_script(&dir, &script);

        let runner = KubectlRunner::new(&binary, "claude-code");
        let result = runner.create_ingress("my-app", 3000).await.unwrap();
        assert!(result.success);

        let yaml = fs::read_to_string(&stdin_file).unwrap();
        assert!(yaml.contains("name: ingress-my-app"), "expected ingress name in yaml: {}", yaml);
        assert!(yaml.contains("host: my-app.localhost"), "expected host in yaml: {}", yaml);
        assert!(yaml.contains("name: svc-my-app"), "expected svc ref in yaml: {}", yaml);
        assert!(yaml.contains("number: 3000"), "expected port number in yaml: {}", yaml);
    }

    #[tokio::test]
    async fn delete_service_ignores_not_found() {
        let dir = TempDir::new().unwrap();
        let args_file = dir.path().join("recorded_args");
        let args_file_str = args_file.to_str().unwrap();
        let script = format!(
            "#!/bin/bash\nprintf '%s ' \"$@\" > \"{}\"\nexit 0\n",
            args_file_str
        );
        let binary = create_mock_script(&dir, &script);

        let runner = KubectlRunner::new(&binary, "claude-code");
        let result = runner.delete_service("my-app").await.unwrap();
        assert!(result.success);

        let recorded = fs::read_to_string(&args_file).unwrap();
        assert!(recorded.contains("--ignore-not-found"), "expected --ignore-not-found in: {}", recorded);
        assert!(recorded.contains("svc-my-app"), "expected svc-my-app in: {}", recorded);
    }

    #[tokio::test]
    async fn delete_ingress_ignores_not_found() {
        let dir = TempDir::new().unwrap();
        let args_file = dir.path().join("recorded_args");
        let args_file_str = args_file.to_str().unwrap();
        let script = format!(
            "#!/bin/bash\nprintf '%s ' \"$@\" > \"{}\"\nexit 0\n",
            args_file_str
        );
        let binary = create_mock_script(&dir, &script);

        let runner = KubectlRunner::new(&binary, "claude-code");
        let result = runner.delete_ingress("my-app").await.unwrap();
        assert!(result.success);

        let recorded = fs::read_to_string(&args_file).unwrap();
        assert!(recorded.contains("--ignore-not-found"), "expected --ignore-not-found in: {}", recorded);
        assert!(recorded.contains("ingress-my-app"), "expected ingress-my-app in: {}", recorded);
    }

    #[tokio::test]
    async fn get_services_parses_json() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            r#"#!/bin/bash
cat << 'ENDJSON'
{
  "items": [
    {
      "metadata": {
        "name": "svc-frontend",
        "labels": {
          "claude-code/project": "frontend"
        }
      }
    },
    {
      "metadata": {
        "name": "svc-backend",
        "labels": {
          "claude-code/project": "backend"
        }
      }
    }
  ]
}
ENDJSON
"#,
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let projects = runner.get_services().await.unwrap();

        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0], "frontend");
        assert_eq!(projects[1], "backend");
    }

    #[tokio::test]
    async fn get_services_empty_list() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            r#"#!/bin/bash
cat << 'ENDJSON'
{"items": []}
ENDJSON
"#,
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let projects = runner.get_services().await.unwrap();
        assert!(projects.is_empty());
    }

    #[tokio::test]
    async fn detect_listening_port_parses_ss() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            "#!/bin/bash\nprintf 'State  Recv-Q Send-Q Local Address:Port\\nLISTEN 0      128    *:3000 *:*\\n'\nexit 0\n",
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let (port, detected) = runner.detect_listening_port("test-pod").await;
        assert_eq!(port, 3000);
        assert!(detected, "should report port as detected");
    }

    #[tokio::test]
    async fn detect_listening_port_fallback() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            "#!/bin/bash\nexit 1\n",
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let (port, detected) = runner.detect_listening_port("test-pod").await;
        assert_eq!(port, 8080);
        assert!(!detected, "should report port as fallback");
    }

    #[tokio::test]
    async fn get_pods_extracts_container_port() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            r#"#!/bin/bash
cat << 'ENDJSON'
{
  "items": [
    {
      "metadata": {
        "name": "web-pod",
        "labels": { "claude-code/project": "web" },
        "creationTimestamp": "2025-03-01T00:00:00Z"
      },
      "status": {
        "phase": "Running",
        "containerStatuses": [{ "ready": true, "restartCount": 0 }]
      },
      "spec": {
        "containers": [{
          "ports": [{ "containerPort": 5000 }]
        }]
      }
    }
  ]
}
ENDJSON
"#,
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let pods = runner.get_pods().await.unwrap();

        assert_eq!(pods.len(), 1);
        assert_eq!(pods[0].container_port, 5000);
    }

    #[tokio::test]
    async fn describe_pod_returns_output() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            "#!/bin/bash\nprintf 'Name: test-pod\\nStatus: Running\\n'\nexit 0\n",
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let result = runner.describe_pod("test-pod").await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("Name: test-pod"));
    }

    // ===== Adversarial / edge-case tests =====

    #[tokio::test]
    async fn detect_listening_port_port_zero_skipped() {
        // ss output where the first port field is :0 (e.g. wildcard) should be skipped
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            "#!/bin/bash\nprintf 'State Recv-Q Send-Q Local\\nLISTEN 0 128 *:0 *:*\\nLISTEN 0 128 *:4000 *:*\\n'\nexit 0\n",
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let (port, detected) = runner.detect_listening_port("pod").await;
        assert_eq!(port, 4000, "should skip port 0 and find 4000");
        assert!(detected, "should report port as detected");
    }

    #[tokio::test]
    async fn detect_listening_port_no_data_lines() {
        // ss output with only a header line and no actual listeners
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            "#!/bin/bash\nprintf 'State Recv-Q Send-Q Local Address:Port\\n'\nexit 0\n",
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let (port, detected) = runner.detect_listening_port("pod").await;
        assert_eq!(port, 8080, "no listeners -> fallback to 8080");
        assert!(!detected, "should report port as fallback");
    }

    #[tokio::test]
    async fn detect_listening_port_garbage_output() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            "#!/bin/bash\nprintf 'header\\nthis is not ss output at all\\nno ports here\\n'\nexit 0\n",
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let (port, detected) = runner.detect_listening_port("pod").await;
        assert_eq!(port, 8080, "garbage output -> fallback to 8080");
        assert!(!detected, "should report port as fallback");
    }

    #[tokio::test]
    async fn detect_listening_port_ipv6_address() {
        // Real ss output sometimes shows :::8080 for IPv6
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            "#!/bin/bash\nprintf 'State Recv-Q Send-Q Local\\nLISTEN 0 128 :::9090 :::*\\n'\nexit 0\n",
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let (port, detected) = runner.detect_listening_port("pod").await;
        assert_eq!(port, 9090, "should parse IPv6 :::PORT format");
        assert!(detected, "should report port as detected");
    }

    #[tokio::test]
    async fn get_services_missing_labels_skipped() {
        // Services where metadata.labels doesn't have claude-code/project should be skipped
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            r#"#!/bin/bash
cat << 'ENDJSON'
{
  "items": [
    {
      "metadata": {
        "name": "svc-good",
        "labels": { "claude-code/project": "myapp" }
      }
    },
    {
      "metadata": {
        "name": "svc-no-label",
        "labels": { "some-other-label": "value" }
      }
    },
    {
      "metadata": {
        "name": "svc-no-labels-at-all"
      }
    }
  ]
}
ENDJSON
"#,
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let projects = runner.get_services().await.unwrap();
        assert_eq!(projects.len(), 1, "only the service with the correct label should be returned");
        assert_eq!(projects[0], "myapp");
    }

    #[tokio::test]
    async fn get_pods_waiting_reason_overrides_phase() {
        // When a container is in waiting state with a reason, that should be the displayed phase
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            r#"#!/bin/bash
cat << 'ENDJSON'
{
  "items": [
    {
      "metadata": {
        "name": "crash-pod",
        "labels": { "claude-code/project": "crasher" },
        "creationTimestamp": "2025-03-01T00:00:00Z"
      },
      "status": {
        "phase": "Running",
        "containerStatuses": [{
          "ready": false,
          "restartCount": 5,
          "state": {
            "waiting": {
              "reason": "CrashLoopBackOff",
              "message": "back-off 5m0s restarting failed container"
            }
          }
        }]
      }
    }
  ]
}
ENDJSON
"#,
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let pods = runner.get_pods().await.unwrap();
        assert_eq!(pods[0].phase, "CrashLoopBackOff", "waiting reason should override phase");
        assert!(!pods[0].warnings.is_empty(), "should have warning from waiting message");
    }

    #[tokio::test]
    async fn get_pods_terminated_completed_not_a_warning() {
        // Terminated with reason "Completed" should NOT generate a warning
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            r#"#!/bin/bash
cat << 'ENDJSON'
{
  "items": [
    {
      "metadata": {
        "name": "done-pod",
        "labels": { "claude-code/project": "done" },
        "creationTimestamp": "2025-03-01T00:00:00Z"
      },
      "status": {
        "phase": "Succeeded",
        "containerStatuses": [{
          "ready": false,
          "restartCount": 0,
          "state": {
            "terminated": { "reason": "Completed", "exitCode": 0 }
          },
          "lastState": {
            "terminated": { "reason": "Completed", "exitCode": 0 }
          }
        }]
      }
    }
  ]
}
ENDJSON
"#,
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let pods = runner.get_pods().await.unwrap();
        assert!(pods[0].warnings.is_empty(), "Completed should not produce warnings, got: {:?}", pods[0].warnings);
    }

    #[tokio::test]
    async fn get_pods_oomkilled_produces_warnings() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            r#"#!/bin/bash
cat << 'ENDJSON'
{
  "items": [
    {
      "metadata": {
        "name": "oom-pod",
        "labels": { "claude-code/project": "oom" },
        "creationTimestamp": "2025-03-01T00:00:00Z"
      },
      "status": {
        "phase": "Running",
        "containerStatuses": [{
          "ready": true,
          "restartCount": 3,
          "state": { "running": {} },
          "lastState": {
            "terminated": { "reason": "OOMKilled", "exitCode": 137 }
          }
        }]
      }
    }
  ]
}
ENDJSON
"#,
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let pods = runner.get_pods().await.unwrap();
        let warnings = &pods[0].warnings;
        assert!(warnings.iter().any(|w| w.contains("OOMKilled")), "should have OOMKilled warning, got: {:?}", warnings);
        assert!(warnings.iter().any(|w| w.contains("137")), "should have exit code 137, got: {:?}", warnings);
    }

    #[tokio::test]
    async fn get_pods_unschedulable_condition() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            r#"#!/bin/bash
cat << 'ENDJSON'
{
  "items": [
    {
      "metadata": {
        "name": "stuck-pod",
        "labels": { "claude-code/project": "stuck" },
        "creationTimestamp": "2025-03-01T00:00:00Z"
      },
      "status": {
        "phase": "Pending",
        "conditions": [
          {
            "type": "PodScheduled",
            "status": "False",
            "reason": "Unschedulable"
          }
        ]
      }
    }
  ]
}
ENDJSON
"#,
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let pods = runner.get_pods().await.unwrap();
        assert!(pods[0].warnings.contains(&"Unschedulable".to_string()),
            "should have Unschedulable warning, got: {:?}", pods[0].warnings);
    }

    #[tokio::test]
    async fn get_pods_malformed_json_returns_error() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            "#!/bin/bash\nprintf '{not valid json'\nexit 0\n",
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let result = runner.get_pods().await;
        assert!(result.is_err(), "malformed JSON should return Err");
    }

    #[tokio::test]
    async fn get_services_malformed_json_returns_error() {
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            "#!/bin/bash\nprintf 'not json at all'\nexit 0\n",
        );

        let runner = KubectlRunner::new(&binary, "claude-code");
        let result = runner.get_services().await;
        assert!(result.is_err(), "malformed JSON should return Err");
    }

    #[test]
    fn truncate_warning_semicolon() {
        let msg = "back-off pulling image; waiting 5m";
        let result = truncate_warning(msg);
        assert_eq!(result, "back-off pulling image");
    }

    #[test]
    fn truncate_warning_second_comma() {
        // First comma is kept, truncates at second comma
        let msg = "path /host/data is not a directory, reason unknown, extra info";
        let result = truncate_warning(msg);
        assert_eq!(result, "path /host/data is not a directory, reason unknown");
    }

    #[test]
    fn truncate_warning_single_comma_kept() {
        let msg = "path /host/data is not a directory, check permissions";
        let result = truncate_warning(msg);
        assert_eq!(result, msg, "single comma should not truncate");
    }

    #[test]
    fn truncate_warning_80_char_limit() {
        let long = "a".repeat(120);
        let result = truncate_warning(&long);
        assert_eq!(result.len(), 80, "should truncate at 80 chars");
    }

    #[test]
    fn truncate_warning_whitespace_trimmed() {
        let msg = "   leading and trailing   ";
        let result = truncate_warning(msg);
        assert_eq!(result, "leading and trailing", "should trim whitespace");
    }

    #[test]
    fn truncate_warning_empty_string() {
        let result = truncate_warning("");
        assert_eq!(result, "");
    }

    #[test]
    fn truncate_warning_semicolon_before_80_chars() {
        let msg = format!("short error; {}", "x".repeat(100));
        let result = truncate_warning(&msg);
        assert_eq!(result, "short error", "semicolon should win over 80-char limit");
    }

    // =========================================================================
    // Pod status edge cases (JSON parsing)
    // =========================================================================

    #[test]
    fn format_age_exactly_one_hour() {
        let now = chrono::Utc::now();
        let ts = now - chrono::Duration::minutes(60);
        let formatted = format_age(&ts.to_rfc3339());
        assert!(formatted.contains("1h"), "expected '1h' in '{}'", formatted);
    }

    #[test]
    fn format_age_exactly_24_hours() {
        let now = chrono::Utc::now();
        let ts = now - chrono::Duration::hours(24);
        let formatted = format_age(&ts.to_rfc3339());
        assert!(formatted.contains("1d"), "expected '1d' in '{}'", formatted);
    }

    #[test]
    fn format_age_very_old() {
        let now = chrono::Utc::now();
        let ts = now - chrono::Duration::days(365);
        let formatted = format_age(&ts.to_rfc3339());
        assert!(formatted.contains("d"), "expected days in '{}'", formatted);
    }

    #[test]
    fn format_age_future_timestamp() {
        let now = chrono::Utc::now();
        let ts = now + chrono::Duration::hours(1);
        let formatted = format_age(&ts.to_rfc3339());
        // Negative duration - should handle gracefully
        assert_eq!(formatted, "< 1m", "future timestamp should show < 1m, got: {}", formatted);
    }

    #[test]
    fn truncate_warning_no_comma_no_semicolon() {
        let msg = "Something went wrong";
        let result = truncate_warning(msg);
        assert_eq!(result, "Something went wrong");
    }

    #[test]
    fn truncate_warning_only_semicolons() {
        let msg = "error; detail; more";
        let result = truncate_warning(msg);
        assert_eq!(result, "error", "should truncate at first semicolon");
    }

    #[test]
    fn truncate_warning_unicode() {
        let msg = "错误信息：容器启动失败";
        let result = truncate_warning(msg);
        assert!(!result.is_empty(), "should handle unicode");
    }

    // =========================================================================
    // Warning clearing logic (enrich_pods_with_events post-processing)
    // =========================================================================

    #[test]
    fn warning_clearing_running_ready_no_restarts() {
        // A healthy pod (Running, ready, 0 restarts) should have warnings cleared
        let mut pod = PodStatus {
            name: "test-pod".into(),
            project: "proj".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 0,
            age: "1h".into(),
            warnings: vec!["FailedMount: /data".into(), "Unhealthy: probe failed".into()],
            exposed: false,
            container_port: 0,
            selected: false,
        };

        // Simulate the clearing logic from enrich_pods_with_events
        if pod.phase == "Running" && pod.ready && pod.restart_count == 0 {
            pod.warnings.clear();
        }
        assert!(pod.warnings.is_empty(), "healthy pod should have warnings cleared");
    }

    #[test]
    fn warning_clearing_running_ready_with_restarts() {
        // A recovered pod with restarts should only keep Last:/Exit: warnings
        let mut pod = PodStatus {
            name: "test-pod".into(),
            project: "proj".into(),
            phase: "Running".into(),
            ready: true,
            restart_count: 3,
            age: "1h".into(),
            warnings: vec![
                "FailedMount: /data".into(),
                "Last: OOMKilled".into(),
                "Exit: 137".into(),
                "CrashBackOff".into(),
            ],
            exposed: false,
            container_port: 0,
            selected: false,
        };

        if pod.phase == "Running" && pod.ready && pod.restart_count == 0 {
            pod.warnings.clear();
        } else if pod.phase == "Running" && pod.ready {
            pod.warnings.retain(|w| w.starts_with("Last:") || w.starts_with("Exit:"));
        }
        assert_eq!(pod.warnings, vec!["Last: OOMKilled", "Exit: 137"]);
    }

    #[test]
    fn warning_clearing_not_ready_keeps_all() {
        // A pod that is not ready should keep all warnings
        let mut pod = PodStatus {
            name: "test-pod".into(),
            project: "proj".into(),
            phase: "Running".into(),
            ready: false,
            restart_count: 0,
            age: "1h".into(),
            warnings: vec!["Unhealthy: readiness probe failed".into()],
            exposed: false,
            container_port: 0,
            selected: false,
        };

        if pod.phase == "Running" && pod.ready && pod.restart_count == 0 {
            pod.warnings.clear();
        }
        assert_eq!(pod.warnings.len(), 1, "not-ready pod should keep warnings");
    }

    #[test]
    fn warning_clearing_pending_keeps_all() {
        let mut pod = PodStatus {
            name: "test-pod".into(),
            project: "proj".into(),
            phase: "Pending".into(),
            ready: false,
            restart_count: 0,
            age: "5m".into(),
            warnings: vec!["Unschedulable".into()],
            exposed: false,
            container_port: 0,
            selected: false,
        };

        if pod.phase == "Running" && pod.ready && pod.restart_count == 0 {
            pod.warnings.clear();
        }
        assert_eq!(pod.warnings.len(), 1, "pending pod should keep warnings");
    }

    #[test]
    fn warning_dedup_prevents_duplicates() {
        // Simulate what enrich_pods_with_events does: only add if not already present
        let mut warnings: Vec<String> = vec!["FailedMount: /data".into()];
        let new_warning = "FailedMount: /data".to_string();
        if !warnings.contains(&new_warning) {
            warnings.push(new_warning);
        }
        assert_eq!(warnings.len(), 1, "duplicate warning should not be added");

        let different_warning = "FailedMount: /config".to_string();
        if !warnings.contains(&different_warning) {
            warnings.push(different_warning);
        }
        assert_eq!(warnings.len(), 2, "different warning should be added");
    }

    // =========================================================================
    // Node status edge cases
    // =========================================================================

    #[test]
    fn node_status_clone() {
        let node = NodeStatus {
            name: "k3d-claude-server-0".to_string(),
            ready: true,
            version: "v1.28.0+k3s1".to_string(),
            age: "2d 3h".to_string(),
            memory_capacity_mb: Some(8000),
            memory_allocatable_mb: Some(7500),
        };
        let cloned = node.clone();
        assert_eq!(node.name, cloned.name);
        assert_eq!(node.ready, cloned.ready);
    }
}

#[cfg(test)]
mod cross_platform_tests {
    use super::*;

    // =========================================================================
    // parse_k8s_memory
    // =========================================================================

    #[test]
    fn parse_k8s_memory_ki_basic() {
        // 8146280Ki => 8146280 / 1024 = 7955 MB
        assert_eq!(parse_k8s_memory("8146280Ki"), Some(7955));
    }

    #[test]
    fn parse_k8s_memory_mi_basic() {
        assert_eq!(parse_k8s_memory("2048Mi"), Some(2048));
    }

    #[test]
    fn parse_k8s_memory_gi_basic() {
        assert_eq!(parse_k8s_memory("4Gi"), Some(4096));
    }

    #[test]
    fn parse_k8s_memory_ti_basic() {
        assert_eq!(parse_k8s_memory("1Ti"), Some(1024 * 1024));
    }

    #[test]
    fn parse_k8s_memory_plain_bytes() {
        // 1073741824 bytes = 1024 MB
        assert_eq!(parse_k8s_memory("1073741824"), Some(1024));
    }

    #[test]
    fn parse_k8s_memory_zero_ki() {
        assert_eq!(parse_k8s_memory("0Ki"), Some(0));
    }

    #[test]
    fn parse_k8s_memory_zero_mi() {
        assert_eq!(parse_k8s_memory("0Mi"), Some(0));
    }

    #[test]
    fn parse_k8s_memory_small_ki_rounds_down() {
        // 512Ki = 0 MB (integer division)
        assert_eq!(parse_k8s_memory("512Ki"), Some(0));
    }

    #[test]
    fn parse_k8s_memory_whitespace_trimmed() {
        assert_eq!(parse_k8s_memory("  4Gi  "), Some(4096));
    }

    #[test]
    fn parse_k8s_memory_invalid_string() {
        assert_eq!(parse_k8s_memory("not-a-number"), None);
    }

    #[test]
    fn parse_k8s_memory_empty_string() {
        assert_eq!(parse_k8s_memory(""), None);
    }

    #[test]
    fn parse_k8s_memory_ki_non_numeric_prefix() {
        assert_eq!(parse_k8s_memory("abcKi"), None);
    }

    #[test]
    fn parse_k8s_memory_plain_bytes_zero() {
        assert_eq!(parse_k8s_memory("0"), Some(0));
    }

    #[test]
    fn parse_k8s_memory_small_plain_bytes() {
        // 500000 bytes < 1MB -> 0
        assert_eq!(parse_k8s_memory("500000"), Some(0));
    }

    // =========================================================================
    // format_age (cross-platform tests for edge cases)
    // =========================================================================

    #[test]
    fn format_age_rfc3339_with_offset() {
        // Should handle timezone offsets via parse_from_rfc3339 fallback
        let now = chrono::Utc::now();
        let ts = now - chrono::Duration::hours(2);
        // Format as RFC3339 with +00:00 suffix
        let ts_str = ts.format("%Y-%m-%dT%H:%M:%S+00:00").to_string();
        let formatted = format_age(&ts_str);
        assert!(formatted.contains("2h"), "expected '2h' in '{}' for input '{}'", formatted, ts_str);
    }

    #[test]
    fn format_age_kubernetes_timestamp_format() {
        // Kubernetes uses ISO 8601 like "2025-01-15T10:30:00Z"
        let now = chrono::Utc::now();
        let ts = now - chrono::Duration::minutes(30);
        let ts_str = ts.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let formatted = format_age(&ts_str);
        assert_eq!(formatted, "30m");
    }

    #[test]
    fn format_age_boundary_59_minutes() {
        let now = chrono::Utc::now();
        let ts = now - chrono::Duration::minutes(59);
        let formatted = format_age(&ts.to_rfc3339());
        assert_eq!(formatted, "59m");
    }

    #[test]
    fn format_age_boundary_exactly_zero() {
        let now = chrono::Utc::now();
        let formatted = format_age(&now.to_rfc3339());
        assert_eq!(formatted, "< 1m");
    }

    // =========================================================================
    // Pod JSON parsing logic (testing the patterns used in get_pods)
    // =========================================================================

    /// Helper: parse a pod JSON item the same way get_pods does.
    fn parse_pod_item(item: &serde_json::Value) -> PodStatus {
        let name = item["metadata"]["name"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let project = item["metadata"]["labels"]["claude-code/project"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let phase = item["status"]["phase"]
            .as_str()
            .unwrap_or("Unknown")
            .to_string();

        let container_statuses = item["status"]["containerStatuses"].as_array();

        let (ready, restart_count) = container_statuses
            .and_then(|cs| cs.first())
            .map(|cs| {
                let ready = cs["ready"].as_bool().unwrap_or(false);
                let restarts = cs["restartCount"].as_u64().unwrap_or(0) as u32;
                (ready, restarts)
            })
            .unwrap_or((false, 0));

        let display_phase = container_statuses
            .and_then(|cs| cs.first())
            .and_then(|cs| {
                cs["state"]["waiting"]["reason"]
                    .as_str()
                    .map(|s| s.to_string())
            })
            .unwrap_or(phase);

        let mut warnings = Vec::new();

        if let Some(cs) = container_statuses.and_then(|cs| cs.first()) {
            if let Some(msg) = cs["state"]["waiting"]["message"].as_str() {
                let short = truncate_warning(msg);
                if !short.is_empty() {
                    warnings.push(short);
                }
            }
            if let Some(reason) = cs["state"]["terminated"]["reason"].as_str() {
                if reason != "Completed" {
                    warnings.push(reason.to_string());
                }
            }
            if let Some(reason) = cs["lastState"]["terminated"]["reason"].as_str() {
                if reason != "Completed" {
                    warnings.push(format!("Last: {}", reason));
                }
            }
            if let Some(exit_code) = cs["lastState"]["terminated"]["exitCode"].as_i64() {
                if exit_code != 0 {
                    warnings.push(format!("Exit: {}", exit_code));
                }
            }
        }

        if let Some(conditions) = item["status"]["conditions"].as_array() {
            for cond in conditions {
                let status = cond["status"].as_str().unwrap_or("");
                let ctype = cond["type"].as_str().unwrap_or("");
                if status == "False" {
                    if let Some(reason) = cond["reason"].as_str() {
                        match reason {
                            "Unschedulable" => warnings.push("Unschedulable".into()),
                            _ if ctype == "PodScheduled" => {
                                warnings.push(format!("Scheduling: {}", reason));
                            }
                            _ if ctype == "Initialized" => {
                                warnings.push(format!("Init: {}", reason));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        let container_port = item["spec"]["containers"]
            .as_array()
            .and_then(|cs| cs.first())
            .and_then(|c| c["ports"].as_array())
            .and_then(|ps| ps.first())
            .and_then(|p| p["containerPort"].as_u64())
            .unwrap_or(0) as u16;

        let age = format_age(
            item["metadata"]["creationTimestamp"]
                .as_str()
                .unwrap_or(""),
        );

        PodStatus {
            name,
            project,
            phase: display_phase,
            ready,
            restart_count,
            age,
            warnings,
            exposed: false,
            container_port,
            selected: false,
        }
    }

    #[test]
    fn parse_pod_running_extracts_phase_and_ready() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {
                "name": "claude-abc123",
                "labels": {"claude-code/project": "myapp"},
                "creationTimestamp": "2025-01-15T10:30:00Z"
            },
            "status": {
                "phase": "Running",
                "containerStatuses": [{"ready": true, "restartCount": 0}]
            },
            "spec": {
                "containers": [{"ports": [{"containerPort": 3000}]}]
            }
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert_eq!(pod.name, "claude-abc123");
        assert_eq!(pod.project, "myapp");
        assert_eq!(pod.phase, "Running");
        assert!(pod.ready);
        assert_eq!(pod.restart_count, 0);
        assert_eq!(pod.container_port, 3000);
        assert!(pod.warnings.is_empty());
    }

    #[test]
    fn parse_pod_pending_not_ready() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {
                "name": "claude-pending",
                "labels": {"claude-code/project": "slowapp"},
                "creationTimestamp": "2025-06-01T00:00:00Z"
            },
            "status": {
                "phase": "Pending",
                "containerStatuses": [{"ready": false, "restartCount": 0}]
            }
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert_eq!(pod.phase, "Pending");
        assert!(!pod.ready);
        assert_eq!(pod.restart_count, 0);
        assert_eq!(pod.container_port, 0);
    }

    #[test]
    fn parse_pod_crashloopbackoff_overrides_phase() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {
                "name": "claude-crash",
                "labels": {"claude-code/project": "broken"},
                "creationTimestamp": "2025-06-01T00:00:00Z"
            },
            "status": {
                "phase": "Running",
                "containerStatuses": [{
                    "ready": false,
                    "restartCount": 8,
                    "state": {
                        "waiting": {
                            "reason": "CrashLoopBackOff",
                            "message": "back-off 5m0s restarting failed container"
                        }
                    }
                }]
            }
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert_eq!(pod.phase, "CrashLoopBackOff");
        assert!(!pod.ready);
        assert_eq!(pod.restart_count, 8);
        assert!(pod.warnings.iter().any(|w| w.contains("back-off")));
    }

    #[test]
    fn parse_pod_image_pull_backoff() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {
                "name": "claude-imgpull",
                "labels": {"claude-code/project": "badimage"},
                "creationTimestamp": "2025-06-01T00:00:00Z"
            },
            "status": {
                "phase": "Pending",
                "containerStatuses": [{
                    "ready": false,
                    "restartCount": 0,
                    "state": {
                        "waiting": {
                            "reason": "ImagePullBackOff",
                            "message": "Back-off pulling image \"nonexistent:latest\""
                        }
                    }
                }]
            }
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert_eq!(pod.phase, "ImagePullBackOff");
        assert!(pod.warnings.iter().any(|w| w.contains("pulling image")));
    }

    #[test]
    fn parse_pod_oomkilled_last_state() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {
                "name": "claude-oom",
                "labels": {"claude-code/project": "hungry"},
                "creationTimestamp": "2025-06-01T00:00:00Z"
            },
            "status": {
                "phase": "Running",
                "containerStatuses": [{
                    "ready": true,
                    "restartCount": 3,
                    "state": {"running": {}},
                    "lastState": {
                        "terminated": {
                            "reason": "OOMKilled",
                            "exitCode": 137
                        }
                    }
                }]
            }
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert_eq!(pod.phase, "Running");
        assert!(pod.ready);
        assert_eq!(pod.restart_count, 3);
        assert!(pod.warnings.iter().any(|w| w == "Last: OOMKilled"));
        assert!(pod.warnings.iter().any(|w| w == "Exit: 137"));
    }

    #[test]
    fn parse_pod_terminated_error_produces_warning() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {
                "name": "claude-err",
                "labels": {"claude-code/project": "errapp"},
                "creationTimestamp": "2025-06-01T00:00:00Z"
            },
            "status": {
                "phase": "Failed",
                "containerStatuses": [{
                    "ready": false,
                    "restartCount": 1,
                    "state": {
                        "terminated": {"reason": "Error", "exitCode": 1}
                    }
                }]
            }
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert!(pod.warnings.iter().any(|w| w == "Error"));
    }

    #[test]
    fn parse_pod_terminated_completed_no_warning() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {
                "name": "claude-done",
                "labels": {"claude-code/project": "doneapp"},
                "creationTimestamp": "2025-06-01T00:00:00Z"
            },
            "status": {
                "phase": "Succeeded",
                "containerStatuses": [{
                    "ready": false,
                    "restartCount": 0,
                    "state": {
                        "terminated": {"reason": "Completed", "exitCode": 0}
                    },
                    "lastState": {
                        "terminated": {"reason": "Completed", "exitCode": 0}
                    }
                }]
            }
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert!(pod.warnings.is_empty(), "Completed should not produce warnings, got: {:?}", pod.warnings);
    }

    #[test]
    fn parse_pod_unschedulable_condition() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {
                "name": "claude-stuck",
                "labels": {"claude-code/project": "stuckapp"},
                "creationTimestamp": "2025-06-01T00:00:00Z"
            },
            "status": {
                "phase": "Pending",
                "conditions": [
                    {"type": "PodScheduled", "status": "False", "reason": "Unschedulable"}
                ]
            }
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert!(pod.warnings.contains(&"Unschedulable".to_string()));
    }

    #[test]
    fn parse_pod_scheduling_failure_condition() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {
                "name": "claude-nosched",
                "labels": {"claude-code/project": "nosched"},
                "creationTimestamp": "2025-06-01T00:00:00Z"
            },
            "status": {
                "phase": "Pending",
                "conditions": [
                    {"type": "PodScheduled", "status": "False", "reason": "InsufficientMemory"}
                ]
            }
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert!(pod.warnings.iter().any(|w| w == "Scheduling: InsufficientMemory"),
            "got: {:?}", pod.warnings);
    }

    #[test]
    fn parse_pod_init_failure_condition() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {
                "name": "claude-initfail",
                "labels": {"claude-code/project": "initfail"},
                "creationTimestamp": "2025-06-01T00:00:00Z"
            },
            "status": {
                "phase": "Pending",
                "conditions": [
                    {"type": "Initialized", "status": "False", "reason": "ContainersNotInitialized"}
                ]
            }
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert!(pod.warnings.iter().any(|w| w == "Init: ContainersNotInitialized"),
            "got: {:?}", pod.warnings);
    }

    #[test]
    fn parse_pod_no_container_statuses() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {
                "name": "claude-init",
                "labels": {"claude-code/project": "initapp"},
                "creationTimestamp": "2025-06-01T00:00:00Z"
            },
            "status": {
                "phase": "Pending"
            }
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert_eq!(pod.phase, "Pending");
        assert!(!pod.ready);
        assert_eq!(pod.restart_count, 0);
        assert_eq!(pod.container_port, 0);
        assert!(pod.warnings.is_empty());
    }

    #[test]
    fn parse_pod_missing_labels_defaults_to_unknown() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {"name": "bare-pod"},
            "status": {"phase": "Running"}
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert_eq!(pod.project, "unknown");
    }

    #[test]
    fn parse_pod_missing_name_defaults_to_unknown() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {},
            "status": {"phase": "Running"}
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert_eq!(pod.name, "unknown");
    }

    #[test]
    fn parse_pod_missing_phase_defaults_to_unknown() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {"name": "test"},
            "status": {}
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert_eq!(pod.phase, "Unknown");
    }

    #[test]
    fn parse_pod_multiple_container_ports_takes_first() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {
                "name": "multi-port",
                "labels": {"claude-code/project": "multiport"},
                "creationTimestamp": "2025-06-01T00:00:00Z"
            },
            "status": {"phase": "Running", "containerStatuses": [{"ready": true, "restartCount": 0}]},
            "spec": {
                "containers": [{
                    "ports": [
                        {"containerPort": 8080},
                        {"containerPort": 9090}
                    ]
                }]
            }
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert_eq!(pod.container_port, 8080, "should take first port");
    }

    #[test]
    fn parse_pod_no_ports_defaults_to_zero() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {
                "name": "no-port",
                "labels": {"claude-code/project": "noport"},
                "creationTimestamp": "2025-06-01T00:00:00Z"
            },
            "status": {"phase": "Running", "containerStatuses": [{"ready": true, "restartCount": 0}]},
            "spec": {"containers": [{}]}
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert_eq!(pod.container_port, 0);
    }

    #[test]
    fn parse_pod_high_restart_count() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {
                "name": "restart-heavy",
                "labels": {"claude-code/project": "restarter"},
                "creationTimestamp": "2025-06-01T00:00:00Z"
            },
            "status": {
                "phase": "Running",
                "containerStatuses": [{"ready": false, "restartCount": 999}]
            }
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert_eq!(pod.restart_count, 999);
    }

    #[test]
    fn parse_pod_last_exit_code_zero_no_warning() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {
                "name": "clean-restart",
                "labels": {"claude-code/project": "clean"},
                "creationTimestamp": "2025-06-01T00:00:00Z"
            },
            "status": {
                "phase": "Running",
                "containerStatuses": [{
                    "ready": true,
                    "restartCount": 1,
                    "state": {"running": {}},
                    "lastState": {
                        "terminated": {"reason": "Completed", "exitCode": 0}
                    }
                }]
            }
        }"#).unwrap();

        let pod = parse_pod_item(&json);
        assert!(pod.warnings.is_empty(),
            "exit code 0 and Completed should produce no warnings, got: {:?}", pod.warnings);
    }

    // =========================================================================
    // Node JSON parsing
    // =========================================================================

    /// Helper: parse a node JSON item the same way get_nodes does.
    fn parse_node_item(item: &serde_json::Value) -> NodeStatus {
        let name = item["metadata"]["name"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let ready = item["status"]["conditions"]
            .as_array()
            .and_then(|conds| {
                conds.iter().find(|c| c["type"].as_str() == Some("Ready"))
            })
            .and_then(|c| c["status"].as_str())
            .map(|s| s == "True")
            .unwrap_or(false);
        let version = item["status"]["nodeInfo"]["kubeletVersion"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let age = item["metadata"]["creationTimestamp"]
            .as_str()
            .map(format_age)
            .unwrap_or_default();
        let memory_capacity_mb = item["status"]["capacity"]["memory"]
            .as_str()
            .and_then(parse_k8s_memory);
        let memory_allocatable_mb = item["status"]["allocatable"]["memory"]
            .as_str()
            .and_then(parse_k8s_memory);
        NodeStatus { name, ready, version, age, memory_capacity_mb, memory_allocatable_mb }
    }

    #[test]
    fn parse_node_ready_with_memory() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {
                "name": "k3d-claude-server-0",
                "creationTimestamp": "2025-01-01T00:00:00Z"
            },
            "status": {
                "conditions": [
                    {"type": "Ready", "status": "True"},
                    {"type": "MemoryPressure", "status": "False"}
                ],
                "nodeInfo": {"kubeletVersion": "v1.28.6+k3s2"},
                "capacity": {"memory": "8146280Ki"},
                "allocatable": {"memory": "7900000Ki"}
            }
        }"#).unwrap();

        let node = parse_node_item(&json);
        assert_eq!(node.name, "k3d-claude-server-0");
        assert!(node.ready);
        assert_eq!(node.version, "v1.28.6+k3s2");
        assert_eq!(node.memory_capacity_mb, Some(7955)); // 8146280 / 1024
        assert_eq!(node.memory_allocatable_mb, Some(7714)); // 7900000 / 1024
    }

    #[test]
    fn parse_node_not_ready() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {"name": "sick-node", "creationTimestamp": "2025-01-01T00:00:00Z"},
            "status": {
                "conditions": [
                    {"type": "Ready", "status": "False"}
                ],
                "nodeInfo": {"kubeletVersion": "v1.27.0"}
            }
        }"#).unwrap();

        let node = parse_node_item(&json);
        assert!(!node.ready);
        assert_eq!(node.version, "v1.27.0");
        assert_eq!(node.memory_capacity_mb, None);
    }

    #[test]
    fn parse_node_no_conditions_defaults_not_ready() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {"name": "bare-node"},
            "status": {}
        }"#).unwrap();

        let node = parse_node_item(&json);
        assert!(!node.ready);
        assert_eq!(node.name, "bare-node");
    }

    #[test]
    fn parse_node_memory_gi_format() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "metadata": {"name": "big-node", "creationTimestamp": "2025-01-01T00:00:00Z"},
            "status": {
                "conditions": [{"type": "Ready", "status": "True"}],
                "nodeInfo": {"kubeletVersion": "v1.28.0"},
                "capacity": {"memory": "16Gi"},
                "allocatable": {"memory": "15Gi"}
            }
        }"#).unwrap();

        let node = parse_node_item(&json);
        assert_eq!(node.memory_capacity_mb, Some(16384));
        assert_eq!(node.memory_allocatable_mb, Some(15360));
    }

    // =========================================================================
    // Service YAML generation
    // =========================================================================

    #[test]
    fn service_yaml_structure() {
        let project = "my-app";
        let port: u16 = 3000;
        let ns = "claude-code";
        let svc_name = format!("svc-{}", project);
        let yaml = format!(
            r#"apiVersion: v1
kind: Service
metadata:
  name: {svc_name}
  namespace: {ns}
  labels:
    claude-code/project: {project}
spec:
  selector:
    claude-code/project: {project}
  ports:
    - port: {port}
      targetPort: {port}
      protocol: TCP
"#,
            svc_name = svc_name,
            ns = ns,
            project = project,
            port = port,
        );

        assert!(yaml.contains("apiVersion: v1"), "missing apiVersion");
        assert!(yaml.contains("kind: Service"), "missing kind");
        assert!(yaml.contains("name: svc-my-app"), "missing service name");
        assert!(yaml.contains("namespace: claude-code"), "missing namespace");
        assert!(yaml.contains("claude-code/project: my-app"), "missing project label");
        assert!(yaml.contains("port: 3000"), "missing port");
        assert!(yaml.contains("targetPort: 3000"), "missing targetPort");
        assert!(yaml.contains("protocol: TCP"), "missing protocol");
        // Verify selector matches labels
        assert!(yaml.matches("claude-code/project: my-app").count() >= 2,
            "project label should appear in both labels and selector");
    }

    #[test]
    fn ingress_yaml_structure() {
        let project = "my-app";
        let port: u16 = 3000;
        let ns = "claude-code";
        let ingress_name = format!("ingress-{}", project);
        let svc_name = format!("svc-{}", project);
        let host = format!("{}.localhost", project);
        let yaml = format!(
            r#"apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: {ingress_name}
  namespace: {ns}
  labels:
    claude-code/project: {project}
spec:
  rules:
    - host: {host}
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: {svc_name}
                port:
                  number: {port}
"#,
            ingress_name = ingress_name,
            ns = ns,
            project = project,
            host = host,
            svc_name = svc_name,
            port = port,
        );

        assert!(yaml.contains("apiVersion: networking.k8s.io/v1"), "missing apiVersion");
        assert!(yaml.contains("kind: Ingress"), "missing kind");
        assert!(yaml.contains("name: ingress-my-app"), "missing ingress name");
        assert!(yaml.contains("namespace: claude-code"), "missing namespace");
        assert!(yaml.contains("host: my-app.localhost"), "missing host");
        assert!(yaml.contains("name: svc-my-app"), "missing service reference");
        assert!(yaml.contains("number: 3000"), "missing port number");
        assert!(yaml.contains("pathType: Prefix"), "missing pathType");
        assert!(yaml.contains("path: /"), "missing path");
    }

    #[test]
    fn service_name_format() {
        let project = "my-long-project-name-123";
        let svc_name = format!("svc-{}", project);
        assert_eq!(svc_name, "svc-my-long-project-name-123");
    }

    #[test]
    fn ingress_name_format() {
        let project = "my-long-project-name-123";
        let ingress_name = format!("ingress-{}", project);
        assert_eq!(ingress_name, "ingress-my-long-project-name-123");
    }

    #[test]
    fn ingress_host_format() {
        let project = "webapp";
        let host = format!("{}.localhost", project);
        assert_eq!(host, "webapp.localhost");
    }

    // =========================================================================
    // Event enrichment parsing patterns
    // =========================================================================

    #[test]
    fn event_warning_matching_failed_mount() {
        let reason = "FailedMount";
        let message = "MountVolume.SetUp failed for volume \"host-data\" ; path /host/data is not a directory";
        let warning = match reason {
            "FailedMount" | "FailedAttachVolume" => {
                Some(format!("FailedMount: {}", truncate_warning(message)))
            }
            _ => None,
        };
        assert!(warning.is_some());
        let w = warning.unwrap();
        assert!(w.starts_with("FailedMount: "));
        // semicolon truncation should stop at "path /host/data is not a directory"
        assert!(!w.contains(';'), "should truncate at semicolon, got: {}", w);
    }

    #[test]
    fn event_warning_matching_backoff() {
        let reason = "BackOff";
        let warning = match reason {
            "BackOff" => Some("CrashBackOff".to_string()),
            _ => None,
        };
        assert_eq!(warning, Some("CrashBackOff".to_string()));
    }

    #[test]
    fn event_warning_matching_insufficient_resources() {
        for reason in &["InsufficientCPU", "InsufficientMemory"] {
            let warning = match *reason {
                "InsufficientCPU" | "InsufficientMemory" => Some(reason.to_string()),
                _ => None,
            };
            assert_eq!(warning, Some(reason.to_string()));
        }
    }

    #[test]
    fn event_warning_matching_unknown_reason_ignored() {
        let reason = "SomeUnknownReason";
        let warning: Option<String> = match reason {
            "FailedMount" | "FailedAttachVolume" => Some("mount".into()),
            "FailedScheduling" => Some("sched".into()),
            "FailedCreate" => Some("create".into()),
            "BackOff" => Some("backoff".into()),
            "Unhealthy" => Some("unhealthy".into()),
            "InsufficientCPU" | "InsufficientMemory" => Some(reason.into()),
            "FailedCreatePodSandBox" => Some("sandbox".into()),
            _ => None,
        };
        assert_eq!(warning, None);
    }

    // =========================================================================
    // Full pod list JSON parsing
    // =========================================================================

    #[test]
    fn parse_pod_list_multiple_pods_various_states() {
        let json_str = r#"{
            "items": [
                {
                    "metadata": {
                        "name": "pod-running",
                        "labels": {"claude-code/project": "app1"},
                        "creationTimestamp": "2025-06-01T00:00:00Z"
                    },
                    "status": {
                        "phase": "Running",
                        "containerStatuses": [{"ready": true, "restartCount": 0}]
                    },
                    "spec": {"containers": [{"ports": [{"containerPort": 3000}]}]}
                },
                {
                    "metadata": {
                        "name": "pod-pending",
                        "labels": {"claude-code/project": "app2"},
                        "creationTimestamp": "2025-06-01T01:00:00Z"
                    },
                    "status": {"phase": "Pending"}
                },
                {
                    "metadata": {
                        "name": "pod-failed",
                        "labels": {"claude-code/project": "app3"},
                        "creationTimestamp": "2025-06-01T02:00:00Z"
                    },
                    "status": {
                        "phase": "Failed",
                        "containerStatuses": [{
                            "ready": false,
                            "restartCount": 5,
                            "state": {"terminated": {"reason": "Error", "exitCode": 1}}
                        }]
                    }
                },
                {
                    "metadata": {
                        "name": "pod-crashloop",
                        "labels": {"claude-code/project": "app4"},
                        "creationTimestamp": "2025-06-01T03:00:00Z"
                    },
                    "status": {
                        "phase": "Running",
                        "containerStatuses": [{
                            "ready": false,
                            "restartCount": 12,
                            "state": {
                                "waiting": {
                                    "reason": "CrashLoopBackOff",
                                    "message": "back-off 5m0s restarting failed container"
                                }
                            },
                            "lastState": {
                                "terminated": {"reason": "OOMKilled", "exitCode": 137}
                            }
                        }]
                    }
                }
            ]
        }"#;

        let pod_list: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let items = pod_list["items"].as_array().unwrap();
        let pods: Vec<PodStatus> = items.iter().map(parse_pod_item).collect();

        assert_eq!(pods.len(), 4);

        // Running pod
        assert_eq!(pods[0].name, "pod-running");
        assert_eq!(pods[0].phase, "Running");
        assert!(pods[0].ready);
        assert_eq!(pods[0].container_port, 3000);

        // Pending pod
        assert_eq!(pods[1].name, "pod-pending");
        assert_eq!(pods[1].phase, "Pending");
        assert!(!pods[1].ready);

        // Failed pod
        assert_eq!(pods[2].name, "pod-failed");
        assert_eq!(pods[2].restart_count, 5);
        assert!(pods[2].warnings.contains(&"Error".to_string()));

        // CrashLoopBackOff pod
        assert_eq!(pods[3].name, "pod-crashloop");
        assert_eq!(pods[3].phase, "CrashLoopBackOff");
        assert_eq!(pods[3].restart_count, 12);
        assert!(pods[3].warnings.iter().any(|w| w == "Last: OOMKilled"));
        assert!(pods[3].warnings.iter().any(|w| w == "Exit: 137"));
    }

    #[test]
    fn parse_pod_list_empty_items() {
        let json_str = r#"{"items": []}"#;
        let pod_list: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let items = pod_list["items"].as_array().unwrap();
        assert!(items.is_empty());
    }

    // =========================================================================
    // truncate_warning additional edge cases
    // =========================================================================

    #[test]
    fn truncate_warning_exactly_80_chars() {
        let msg = "a".repeat(80);
        let result = truncate_warning(&msg);
        assert_eq!(result.len(), 80);
    }

    #[test]
    fn truncate_warning_79_chars_no_truncation() {
        let msg = "a".repeat(79);
        let result = truncate_warning(&msg);
        assert_eq!(result.len(), 79);
    }

    #[test]
    fn truncate_warning_real_k8s_message() {
        let msg = "Back-off pulling image \"registry.example.com/myapp:v1.0.0\"; waiting 2m30s";
        let result = truncate_warning(msg);
        assert_eq!(result, "Back-off pulling image \"registry.example.com/myapp:v1.0.0\"");
    }

    #[test]
    fn truncate_warning_mount_error_with_commas() {
        let msg = "MountVolume.SetUp failed for volume \"data\", path /host/data, reason: not a directory";
        let result = truncate_warning(msg);
        // Should keep first comma, truncate at second
        assert_eq!(result, "MountVolume.SetUp failed for volume \"data\", path /host/data");
    }

    // =========================================================================
    // extract_describe_events
    // =========================================================================

    #[test]
    fn extract_describe_events_last_state() {
        let describe = "\
Name: test-pod
    Last State:     Terminated
      Reason:       OOMKilled
      Exit Code:    137
    Ready:          True
";
        let result = extract_describe_events(describe);
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("Last State:"), "should contain Last State header");
        assert!(text.contains("OOMKilled"), "should contain termination reason");
    }

    #[test]
    fn extract_describe_events_events_section() {
        let describe = "\
Name: test-pod
Status: Running
Events:
  Type    Reason   Age   Message
  ----    ------   ---   -------
  Normal  Pulled   5m    Container image pulled
  Warning BackOff  1m    Back-off restarting failed container
";
        let result = extract_describe_events(describe);
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("Events:"), "should contain Events header");
        assert!(text.contains("BackOff"), "should contain event reason");
    }

    #[test]
    fn extract_describe_events_both_sections() {
        let describe = "\
Name: test-pod
    Last State:     Terminated
      Reason:       Error
    Ready:          False
Events:
  Type    Reason   Age   Message
  Normal  Pulled   5m    Image pulled
";
        let result = extract_describe_events(describe);
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("Last State:"), "should contain Last State");
        assert!(text.contains("Events:"), "should contain Events");
    }

    #[test]
    fn extract_describe_events_empty() {
        let describe = "\
Name: test-pod
Status: Running
Containers:
  main:
    Image: node:22
";
        let result = extract_describe_events(describe);
        assert!(result.is_none(), "should return None when no events or last state");
    }

    #[test]
    fn extract_describe_events_at_start_of_output() {
        // Events: at the very beginning (no leading newline) should NOT be found
        // because the code searches for "\nEvents:"
        let describe = "Events:\n  Type  Reason  Age  Message\n  Normal  Pulled  5m  ok\n";
        let result = extract_describe_events(describe);
        assert!(result.is_none(),
            "Events: at start of output (no leading newline) is not captured by current impl");
    }

    #[test]
    fn extract_describe_events_header_only() {
        // Events section present but with no actual event rows
        let describe = "\
Name: test-pod
Events:
  <none>
";
        let result = extract_describe_events(describe);
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("Events:"));
        assert!(text.contains("<none>"));
    }

    #[test]
    fn extract_describe_events_multiple_last_states() {
        // Two containers each with Last State (only first should be captured because
        // the parser exits in_last_state when indent decreases)
        let describe = "\
    Last State:     Terminated
      Reason:       OOMKilled
      Exit Code:    137
    Ready:          True
    Last State:     Terminated
      Reason:       Error
      Exit Code:    1
    Ready:          False
";
        let result = extract_describe_events(describe).unwrap();
        assert!(result.contains("OOMKilled"), "first Last State should be captured");
        assert!(result.contains("Error"), "second Last State should also be captured");
    }

    #[test]
    fn extract_describe_events_last_state_no_children() {
        // Last State line with no indented children following it
        let describe = "\
    Last State:     Waiting
    Ready:          False
";
        let result = extract_describe_events(describe);
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("Last State:"), "header line itself should be captured");
    }

    #[test]
    fn extract_describe_events_empty_string() {
        let result = extract_describe_events("");
        assert!(result.is_none());
    }

    #[test]
    fn extract_describe_events_events_with_newline_in_middle() {
        // "\nEvents:" appears but buried in the middle, not at the end
        let describe = "\
Name: test-pod
Events:
  Type    Reason   Age  Message
  Normal  Pulled   5m   ok
Other stuff after events
";
        let result = extract_describe_events(describe).unwrap();
        // The code takes everything from "\nEvents:" to end, so "Other stuff" is included
        assert!(result.contains("Events:"));
        assert!(result.contains("Other stuff"), "everything after Events: is captured");
    }

    #[test]
    fn extract_describe_events_only_whitespace() {
        let result = extract_describe_events("   \n  \n  ");
        assert!(result.is_none());
    }

    #[test]
    fn extract_describe_events_last_state_with_oomkilled_and_events() {
        let describe = "\
Name: test-pod
Containers:
  main:
    State:          Running
    Last State:     Terminated
      Reason:       OOMKilled
      Exit Code:    137
      Started:      Mon, 01 Jan 2025 00:00:00 +0000
      Finished:     Mon, 01 Jan 2025 01:00:00 +0000
    Ready:          True
Events:
  Type     Reason     Age   Message
  ----     ------     ---   -------
  Warning  OOMKilling 5m    Memory cgroup out of memory
";
        let result = extract_describe_events(describe);
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("OOMKilled"), "should have OOMKilled from Last State");
        assert!(text.contains("OOMKilling"), "should have OOMKilling from Events");
    }

    #[test]
    fn extract_describe_events_events_with_no_events_marker() {
        let describe = "\
Name: test-pod
Events:  <none>
";
        let result = extract_describe_events(describe);
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("<none>"));
    }
}

#[cfg(all(test, unix))]
mod log_and_delete_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_mock_script(dir: &TempDir, content: &str) -> String {
        let script_path = dir.path().join("kubectl");
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(0o755)
                .open(&script_path)
                .unwrap();
            f.write_all(content.as_bytes()).unwrap();
            f.sync_all().unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        script_path.to_str().unwrap().to_string()
    }

    // =========================================================================
    // get_logs: current logs only (no previous container) (POD-15)
    // =========================================================================

    #[tokio::test]
    async fn get_logs_current_only_no_previous() {
        // Mock: "logs" succeeds, "--previous" fails (no previous container)
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            r#"#!/bin/bash
case "$@" in
    *--previous*) exit 1;;
    *logs*) printf 'current log line 1\ncurrent log line 2\n';;
    *) exit 1;;
esac
"#,
        );

        let runner = KubectlRunner::new(&binary, "test-ns");
        let result = runner.get_logs("my-pod", 100).await.unwrap();
        assert!(result.success);
        assert_eq!(result.stdout, "current log line 1\ncurrent log line 2\n");
        // Should NOT contain the separator since there are no previous logs
        assert!(!result.stdout.contains("=== Previous container logs"),
            "should not have previous log separator when no previous logs exist");
    }

    // =========================================================================
    // get_logs: previous + current logs with separator (POD-16)
    // =========================================================================

    #[tokio::test]
    async fn get_logs_previous_and_current_with_separator() {
        // Mock: both "logs" and "logs --previous" succeed
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            r#"#!/bin/bash
case "$@" in
    *--previous*) printf 'prev line 1\nprev line 2\n';;
    *logs*) printf 'current line 1\ncurrent line 2\n';;
    *) exit 1;;
esac
"#,
        );

        let runner = KubectlRunner::new(&binary, "test-ns");
        let result = runner.get_logs("my-pod", 50).await.unwrap();
        assert!(result.success);
        // Verify the combined output has the separator format
        assert!(result.stdout.contains("=== Previous container logs (crash reason) ==="),
            "should contain previous logs header, got: {}", result.stdout);
        assert!(result.stdout.contains("prev line 1"),
            "should contain previous log content");
        assert!(result.stdout.contains("=== Current container logs ==="),
            "should contain current logs header");
        assert!(result.stdout.contains("current line 1"),
            "should contain current log content");

        // Verify order: previous logs come before current logs
        let prev_pos = result.stdout.find("prev line 1").unwrap();
        let curr_pos = result.stdout.find("current line 1").unwrap();
        assert!(prev_pos < curr_pos,
            "previous logs should appear before current logs");
    }

    // =========================================================================
    // get_logs: falls back to describe when logs fail (POD-15)
    // =========================================================================

    #[tokio::test]
    async fn get_logs_falls_back_to_describe() {
        // Mock: "logs" fails, "describe" succeeds
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            r#"#!/bin/bash
case "$@" in
    *describe*) printf 'Name: my-pod\nStatus: Pending\nEvents:\n  Waiting for image pull\n';;
    *logs*) exit 1;;
    *) exit 1;;
esac
"#,
        );

        let runner = KubectlRunner::new(&binary, "test-ns");
        let result = runner.get_logs("my-pod", 100).await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("Name: my-pod"),
            "should contain describe output when logs fail, got: {}", result.stdout);
        assert!(result.stdout.contains("Events:"),
            "should contain Events section from describe");
    }

    // =========================================================================
    // get_logs: both logs and describe fail (POD-15)
    // =========================================================================

    #[tokio::test]
    async fn get_logs_both_fail() {
        // Mock: everything fails
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            "#!/bin/bash\nexit 1\n",
        );

        let runner = KubectlRunner::new(&binary, "test-ns");
        let result = runner.get_logs("my-pod", 100).await.unwrap();
        // When logs fail, it falls back to describe. When describe also fails,
        // the result comes from describe (which also failed).
        assert!(!result.success, "should report failure when both logs and describe fail");
    }

    // =========================================================================
    // get_logs: previous logs empty are ignored
    // =========================================================================

    #[tokio::test]
    async fn get_logs_previous_empty_ignored() {
        // Mock: previous logs command succeeds but returns empty output
        let dir = TempDir::new().unwrap();
        let binary = create_mock_script(
            &dir,
            r#"#!/bin/bash
case "$@" in
    *--previous*) printf '\n  \n';;
    *logs*) printf 'current output here\n';;
    *) exit 1;;
esac
"#,
        );

        let runner = KubectlRunner::new(&binary, "test-ns");
        let result = runner.get_logs("my-pod", 50).await.unwrap();
        assert!(result.success);
        // Empty/whitespace previous logs should be ignored (no separator added)
        assert!(!result.stdout.contains("=== Previous container logs"),
            "should not show separator when previous logs are empty/whitespace");
        assert_eq!(result.stdout, "current output here\n");
    }

    // =========================================================================
    // delete_service: verifies exact kubectl command args (POD-35)
    // =========================================================================

    #[tokio::test]
    async fn delete_service_verifies_args() {
        let dir = TempDir::new().unwrap();
        let args_file = dir.path().join("recorded_args");
        let args_file_str = args_file.to_str().unwrap();
        let script = format!(
            "#!/bin/bash\nprintf '%s ' \"$@\" > \"{}\"\nexit 0\n",
            args_file_str
        );
        let binary = create_mock_script(&dir, &script);

        let runner = KubectlRunner::new(&binary, "claude-code");
        runner.delete_service("my-project").await.unwrap();

        let recorded = fs::read_to_string(&args_file).unwrap();
        let recorded = recorded.trim();
        assert_eq!(recorded, "delete svc svc-my-project -n claude-code --ignore-not-found",
            "delete_service should pass correct args, got: {}", recorded);
    }

    // =========================================================================
    // delete_ingress: verifies exact kubectl command args (POD-35)
    // =========================================================================

    #[tokio::test]
    async fn delete_ingress_verifies_args() {
        let dir = TempDir::new().unwrap();
        let args_file = dir.path().join("recorded_args");
        let args_file_str = args_file.to_str().unwrap();
        let script = format!(
            "#!/bin/bash\nprintf '%s ' \"$@\" > \"{}\"\nexit 0\n",
            args_file_str
        );
        let binary = create_mock_script(&dir, &script);

        let runner = KubectlRunner::new(&binary, "claude-code");
        runner.delete_ingress("my-project").await.unwrap();

        let recorded = fs::read_to_string(&args_file).unwrap();
        let recorded = recorded.trim();
        assert_eq!(recorded, "delete ingress ingress-my-project -n claude-code --ignore-not-found",
            "delete_ingress should pass correct args, got: {}", recorded);
    }

    // =========================================================================
    // delete_service: resource name format
    // =========================================================================

    #[tokio::test]
    async fn delete_service_uses_svc_prefix() {
        let dir = TempDir::new().unwrap();
        let args_file = dir.path().join("recorded_args");
        let args_file_str = args_file.to_str().unwrap();
        let script = format!(
            "#!/bin/bash\nprintf '%s ' \"$@\" > \"{}\"\nexit 0\n",
            args_file_str
        );
        let binary = create_mock_script(&dir, &script);

        let runner = KubectlRunner::new(&binary, "ns");
        runner.delete_service("frontend").await.unwrap();

        let recorded = fs::read_to_string(&args_file).unwrap();
        assert!(recorded.contains("svc-frontend"),
            "service name should be prefixed with 'svc-', got: {}", recorded);
    }

    // =========================================================================
    // delete_ingress: resource name format
    // =========================================================================

    #[tokio::test]
    async fn delete_ingress_uses_ingress_prefix() {
        let dir = TempDir::new().unwrap();
        let args_file = dir.path().join("recorded_args");
        let args_file_str = args_file.to_str().unwrap();
        let script = format!(
            "#!/bin/bash\nprintf '%s ' \"$@\" > \"{}\"\nexit 0\n",
            args_file_str
        );
        let binary = create_mock_script(&dir, &script);

        let runner = KubectlRunner::new(&binary, "ns");
        runner.delete_ingress("frontend").await.unwrap();

        let recorded = fs::read_to_string(&args_file).unwrap();
        assert!(recorded.contains("ingress-frontend"),
            "ingress name should be prefixed with 'ingress-', got: {}", recorded);
    }
}
