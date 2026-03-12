use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum AppError {
    #[error("Terraform error: {0}")]
    Terraform(String),

    #[error("Helm error: {0}")]
    Helm(String),

    #[error("Kubectl error: {0}")]
    Kubectl(String),

    #[error("Docker error: {0}")]
    Docker(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Project scan error: {0}")]
    ProjectScan(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Platform error: {0}")]
    Platform(String),
}

pub type AppResult<T> = Result<T, AppError>;

/// Shared result type for CLI command execution
#[derive(Debug, Clone)]
pub struct CmdResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl CmdResult {
    /// Format a command result for display in logs.
    ///
    /// # Arguments
    ///
    /// * `cmd` - The command name or description to include in the output
    ///
    /// # Returns
    ///
    /// A formatted string with status, stdout (if non-empty), and stderr (if non-empty).
    pub fn format(&self, cmd: &str) -> String {
        let status = if self.success { "SUCCESS" } else { "FAILED" };
        let mut out = format!("[{}] {}", status, cmd);
        if !self.stdout.is_empty() {
            out.push_str(&format!("\n{}", self.stdout.trim()));
        }
        if !self.stderr.is_empty() {
            out.push_str(&format!("\nSTDERR: {}", self.stderr.trim()));
        }
        out
    }
}

/// Format a command result for display in logs (convenience wrapper).
///
/// # Arguments
///
/// * `cmd` - The command name or description to include in the output
/// * `r` - The command result to format
///
/// # Returns
///
/// A formatted string with status, stdout (if non-empty), and stderr (if non-empty).
pub fn format_cmd_result(cmd: &str, r: &CmdResult) -> String {
    r.format(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_result_success() {
        let result = CmdResult {
            success: true,
            stdout: "all good".to_string(),
            stderr: String::new(),
        };
        assert!(result.success);
        assert_eq!(result.stdout, "all good");
        assert_eq!(result.stderr, "");
    }

    #[test]
    fn cmd_result_failure() {
        let result = CmdResult {
            success: false,
            stdout: String::new(),
            stderr: "something went wrong".to_string(),
        };
        assert!(!result.success);
        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr, "something went wrong");
    }

    #[test]
    fn cmd_result_clone() {
        let original = CmdResult {
            success: true,
            stdout: "output".to_string(),
            stderr: "warnings".to_string(),
        };
        let cloned = original.clone();
        assert_eq!(original.success, cloned.success);
        assert_eq!(original.stdout, cloned.stdout);
        assert_eq!(original.stderr, cloned.stderr);
    }

    #[test]
    fn app_error_display_messages() {
        let cases: Vec<(AppError, &str)> = vec![
            (AppError::Terraform("tf failed".into()), "Terraform error: tf failed"),
            (AppError::Helm("helm failed".into()), "Helm error: helm failed"),
            (AppError::Kubectl("kubectl failed".into()), "Kubectl error: kubectl failed"),
            (AppError::Docker("docker failed".into()), "Docker error: docker failed"),
            (AppError::Config("bad config".into()), "Configuration error: bad config"),
            (AppError::ProjectScan("scan failed".into()), "Project scan error: scan failed"),
            (AppError::Platform("unsupported".into()), "Platform error: unsupported"),
        ];
        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn app_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let app_err: AppError = io_err.into();
        let msg = app_err.to_string();
        assert!(msg.starts_with("IO error:"));
        assert!(msg.contains("file missing"));
    }

    #[test]
    fn app_error_from_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
        let app_err: AppError = json_err.into();
        let msg = app_err.to_string();
        assert!(msg.starts_with("JSON error:"));
    }

    #[test]
    fn app_result_ok() {
        let result: AppResult<i32> = Ok(42);
        assert!(result.is_ok());
        assert_eq!(*result.as_ref().unwrap(), 42);
    }

    #[test]
    fn app_result_err() {
        let result: AppResult<i32> = Err(AppError::Config("missing key".into()));
        assert!(result.is_err());
        assert_eq!(
            result.as_ref().unwrap_err().to_string(),
            "Configuration error: missing key"
        );
    }

    // =========================================================================
    // format_cmd_result / CmdResult::format
    // =========================================================================

    #[test]
    fn format_cmd_result_success() {
        let r = CmdResult {
            success: true,
            stdout: "all good".to_string(),
            stderr: String::new(),
        };
        let output = format_cmd_result("terraform apply", &r);
        assert!(output.contains("[SUCCESS]"), "expected [SUCCESS] in: {}", output);
        assert!(output.contains("terraform apply"), "expected command name in: {}", output);
        assert!(output.contains("all good"), "expected stdout in: {}", output);
    }

    #[test]
    fn format_cmd_result_failure() {
        let r = CmdResult {
            success: false,
            stdout: String::new(),
            stderr: "something went wrong".to_string(),
        };
        let output = format_cmd_result("terraform apply", &r);
        assert!(output.contains("[FAILED]"), "expected [FAILED] in: {}", output);
        assert!(
            output.contains("STDERR: something went wrong"),
            "expected stderr in: {}",
            output
        );
    }

    #[test]
    fn format_cmd_result_both_stdout_stderr() {
        let r = CmdResult {
            success: true,
            stdout: "some output".to_string(),
            stderr: "some warning".to_string(),
        };
        let output = format_cmd_result("helm install", &r);
        assert!(output.contains("some output"), "expected stdout in: {}", output);
        assert!(
            output.contains("STDERR: some warning"),
            "expected stderr in: {}",
            output
        );
    }

    #[test]
    fn format_cmd_result_empty_output() {
        let r = CmdResult {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        };
        let output = format_cmd_result("test cmd", &r);
        assert_eq!(output, "[SUCCESS] test cmd");
    }

    #[test]
    fn format_cmd_result_whitespace_trimmed() {
        let r = CmdResult {
            success: false,
            stdout: "  leading and trailing  \n\n".to_string(),
            stderr: "  warn with spaces  \n".to_string(),
        };
        let output = format_cmd_result("cmd", &r);
        assert!(
            output.contains("leading and trailing"),
            "expected trimmed stdout in: {}",
            output
        );
        assert!(
            !output.contains("leading and trailing  \n"),
            "stdout should be trimmed, got: {}",
            output
        );
    }

    #[test]
    fn format_cmd_result_very_long_stderr() {
        let r = CmdResult {
            success: false,
            stdout: String::new(),
            stderr: "x".repeat(10000),
        };
        let output = format_cmd_result("cmd", &r);
        assert!(output.contains("STDERR:"), "should contain STDERR marker");
    }

    #[test]
    fn format_cmd_result_multiline_output() {
        let r = CmdResult {
            success: true,
            stdout: "line1\nline2\nline3".to_string(),
            stderr: String::new(),
        };
        let output = format_cmd_result("cmd", &r);
        assert!(output.contains("line1\nline2\nline3"));
    }

    #[test]
    fn cmd_result_format_method_matches_standalone() {
        let r = CmdResult {
            success: true,
            stdout: "output".to_string(),
            stderr: "warning".to_string(),
        };
        assert_eq!(r.format("test"), format_cmd_result("test", &r));
    }
}
