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

                let age = format_age(
                    item["metadata"]["creationTimestamp"]
                        .as_str()
                        .unwrap_or(""),
                );

                pods.push(PodStatus {
                    name,
                    project,
                    phase,
                    ready,
                    restart_count,
                    age,
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
        self.run(&["logs", "-n", &self.namespace, pod_name, "--tail", &tail])
            .await
    }

    pub async fn cluster_health(&self) -> AppResult<bool> {
        let result = self.run(&["cluster-info"]).await?;
        Ok(result.success)
    }

    pub async fn delete_pod(&self, pod_name: &str) -> AppResult<CmdResult> {
        self.run(&["delete", "pod", "-n", &self.namespace, pod_name])
            .await
    }
}

#[cfg(test)]
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
        };

        let cloned = pod.clone();

        assert_eq!(pod.name, cloned.name);
        assert_eq!(pod.project, cloned.project);
        assert_eq!(pod.phase, cloned.phase);
        assert_eq!(pod.ready, cloned.ready);
        assert_eq!(pod.restart_count, cloned.restart_count);
        assert_eq!(pod.age, cloned.age);
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
