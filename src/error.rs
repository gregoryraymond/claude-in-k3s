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
}
