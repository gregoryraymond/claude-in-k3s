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
}

#[derive(Debug, Clone)]
pub struct NodeStatus {
    #[allow(dead_code)]
    pub name: String,
    pub ready: bool,
}

pub struct KubectlRunner {
    binary: String,
    namespace: String,
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
                });
            }
        }

        Ok(pods)
    }

    pub async fn exec_claude(&self, pod_name: &str, prompt: &str) -> AppResult<CmdResult> {
        self.run(&[
            "exec",
            "-n",
            &self.namespace,
            pod_name,
            "--",
            "claude",
            "-p",
            "--dangerously-skip-permissions",
            "--output-format",
            "stream-json",
            "--",
            prompt,
        ])
        .await
    }

    pub async fn get_logs(&self, pod_name: &str, tail_lines: u32) -> AppResult<CmdResult> {
        let tail = tail_lines.to_string();
        let result = self
            .run(&["logs", "-n", &self.namespace, pod_name, "--tail", &tail])
            .await?;

        // If logs failed (e.g. container not started), fall back to describe
        if !result.success {
            return self.describe_pod(pod_name).await;
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
                nodes.push(NodeStatus { name, ready });
            }
        }

        Ok(nodes)
    }

    pub async fn delete_pod(&self, pod_name: &str) -> AppResult<CmdResult> {
        self.run(&["delete", "pod", "-n", &self.namespace, pod_name])
            .await
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

        Ok(())
    }
}

/// Truncate a warning message to a reasonable badge length.
fn truncate_warning(msg: &str) -> String {
    // Take first meaningful clause (up to first comma, semicolon, or 80 chars)
    let trimmed = msg.trim();
    let end = trimmed
        .find(|c: char| c == ';')
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
        };

        let cloned = pod.clone();

        assert_eq!(pod.name, cloned.name);
        assert_eq!(pod.project, cloned.project);
        assert_eq!(pod.phase, cloned.phase);
        assert_eq!(pod.ready, cloned.ready);
        assert_eq!(pod.restart_count, cloned.restart_count);
        assert_eq!(pod.age, cloned.age);
        assert_eq!(pod.warnings, cloned.warnings);
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
}
