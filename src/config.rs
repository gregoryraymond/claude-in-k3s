use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub projects_dir: Option<String>,
    pub terraform_dir: String,
    pub helm_chart_dir: String,
    pub claude_mode: String,
    pub git_user_name: String,
    pub git_user_email: String,
    pub cpu_limit: String,
    pub memory_limit: String,
    #[serde(default = "default_cluster_memory_percent")]
    pub cluster_memory_percent: u8,
    /// Extra host paths to mount into pods (SET-12/SET-13).
    #[serde(default)]
    pub extra_mounts: Vec<String>,
    /// Docker build timeout in seconds (SET-17).
    #[serde(default = "default_build_timeout")]
    pub build_timeout_secs: u64,
    /// Helm deploy timeout in seconds (SET-18).
    #[serde(default = "default_deploy_timeout")]
    pub deploy_timeout_secs: u64,
    /// Image retention period in hours (SET-15). 0 = no cleanup.
    #[serde(default = "default_image_retention_hours")]
    pub image_retention_hours: u64,
}

fn default_cluster_memory_percent() -> u8 {
    80
}

fn default_build_timeout() -> u64 {
    600 // 10 minutes
}

fn default_deploy_timeout() -> u64 {
    300 // 5 minutes
}

fn default_image_retention_hours() -> u64 {
    168 // 7 days
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            projects_dir: None,
            terraform_dir: "terraform".into(),
            helm_chart_dir: "helm/claude-code".into(),
            claude_mode: "daemon".into(),
            git_user_name: "Claude Code Bot".into(),
            git_user_email: "claude-bot@localhost".into(),
            cpu_limit: "2".into(),
            memory_limit: "4Gi".into(),
            cluster_memory_percent: 80,
            extra_mounts: Vec::new(),
            build_timeout_secs: default_build_timeout(),
            deploy_timeout_secs: default_deploy_timeout(),
            image_retention_hours: default_image_retention_hours(),
        }
    }
}

impl AppConfig {
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("claude-in-k3s")
            .join("config.toml")
    }

    pub fn load() -> AppResult<Self> {
        let path = Self::config_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            match toml::from_str(&content) {
                Ok(cfg) => Ok(cfg),
                Err(e) => {
                    // APP-16: Fall back to defaults on corrupt config, overwrite the bad file
                    tracing::warn!("Config file corrupt, falling back to defaults: {}", e);
                    let defaults = Self::default();
                    if let Err(save_err) = defaults.save() {
                        tracing::warn!("Failed to overwrite corrupt config: {}", save_err);
                    }
                    Ok(defaults)
                }
            }
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> AppResult<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content =
            toml::to_string_pretty(self).map_err(|e| AppError::Config(e.to_string()))?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Load config from an arbitrary path (returns default if file does not exist or is corrupt).
    pub fn load_from(path: &std::path::Path) -> AppResult<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            match toml::from_str(&content) {
                Ok(cfg) => Ok(cfg),
                Err(_e) => {
                    // APP-16: Fall back to defaults on corrupt config
                    Ok(Self::default())
                }
            }
        } else {
            Ok(Self::default())
        }
    }

    /// SET-19: Validate config values before saving.
    /// Returns a list of error messages, empty if valid.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if self.cpu_limit.is_empty() {
            errors.push("CPU limit cannot be empty.".into());
        } else if self.cpu_limit.parse::<f64>().is_err() {
            errors.push(format!("CPU limit '{}' is not a valid number.", self.cpu_limit));
        }

        if self.memory_limit.is_empty() {
            errors.push("Memory limit cannot be empty.".into());
        } else if !self.memory_limit.ends_with("Gi") && !self.memory_limit.ends_with("Mi") {
            // Accept numeric-only as megabytes, or standard K8s suffixes
            if self.memory_limit.parse::<u64>().is_err() {
                errors.push(format!(
                    "Memory limit '{}' must end with 'Gi' or 'Mi', or be a plain number.",
                    self.memory_limit
                ));
            }
        }

        if self.git_user_name.trim().is_empty() {
            errors.push("Git user name cannot be empty.".into());
        }

        if self.git_user_email.trim().is_empty() {
            errors.push("Git user email cannot be empty.".into());
        }

        if self.terraform_dir.trim().is_empty() {
            errors.push("Terraform directory cannot be empty.".into());
        }

        if self.helm_chart_dir.trim().is_empty() {
            errors.push("Helm chart directory cannot be empty.".into());
        }

        errors
    }

    /// Save config to an arbitrary path.
    pub fn save_to(&self, path: &std::path::Path) -> AppResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content =
            toml::to_string_pretty(self).map_err(|e| AppError::Config(e.to_string()))?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

/// Parse a K8s memory limit string (e.g. "4Gi", "512Mi", "4096") to megabytes.
///
/// # Arguments
///
/// * `s` - A string slice containing a Kubernetes memory limit value
///
/// # Returns
///
/// The memory limit in megabytes. Falls back to sensible defaults on parse failure:
/// 4 * 1024 = 4096 for Gi, 512 for Mi, and 4096 for plain numbers.
pub fn parse_memory_limit_mb(s: &str) -> u64 {
    let s = s.trim();
    if s.ends_with("Gi") {
        s.trim_end_matches("Gi").parse::<u64>().unwrap_or(4) * 1024
    } else if s.ends_with("Mi") {
        s.trim_end_matches("Mi").parse::<u64>().unwrap_or(512)
    } else {
        s.parse::<u64>().unwrap_or(4096)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_config_values() {
        let cfg = AppConfig::default();
        assert!(cfg.projects_dir.is_none());
        assert_eq!(cfg.terraform_dir, "terraform");
        assert_eq!(cfg.helm_chart_dir, "helm/claude-code");
        assert_eq!(cfg.claude_mode, "daemon");
        assert_eq!(cfg.git_user_name, "Claude Code Bot");
        assert_eq!(cfg.git_user_email, "claude-bot@localhost");
        assert_eq!(cfg.cpu_limit, "2");
        assert_eq!(cfg.memory_limit, "4Gi");
        assert!(cfg.extra_mounts.is_empty());
        assert_eq!(cfg.build_timeout_secs, 600);
        assert_eq!(cfg.deploy_timeout_secs, 300);
        assert_eq!(cfg.image_retention_hours, 168); // SET-15: 7 days default
    }

    #[test]
    fn config_path_is_under_config_dir() {
        let path = AppConfig::config_path();
        // The path must end with claude-in-k3s/config.toml regardless of
        // the platform-specific config directory prefix.
        assert!(path.ends_with("claude-in-k3s/config.toml"));
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let cfg = AppConfig {
            projects_dir: Some("/tmp/projects".into()),
            terraform_dir: "my-tf".into(),
            helm_chart_dir: "charts/claude".into(),
            claude_mode: "interactive".into(),
            git_user_name: "Test User".into(),
            git_user_email: "test@example.com".into(),
            cpu_limit: "4".into(),
            memory_limit: "8Gi".into(),
            cluster_memory_percent: 90,
            ..Default::default()
        };

        let toml_str = toml::to_string_pretty(&cfg).expect("serialize");
        let loaded: AppConfig = toml::from_str(&toml_str).expect("deserialize");

        assert_eq!(loaded.projects_dir.as_deref(), Some("/tmp/projects"));
        assert_eq!(loaded.terraform_dir, "my-tf");
        assert_eq!(loaded.helm_chart_dir, "charts/claude");
        assert_eq!(loaded.claude_mode, "interactive");
        assert_eq!(loaded.git_user_name, "Test User");
        assert_eq!(loaded.git_user_email, "test@example.com");
        assert_eq!(loaded.cpu_limit, "4");
        assert_eq!(loaded.memory_limit, "8Gi");
        assert_eq!(loaded.cluster_memory_percent, 90);
    }

    #[test]
    fn serialize_with_none_fields() {
        let cfg = AppConfig {
            projects_dir: None,
            ..AppConfig::default()
        };

        let toml_str = toml::to_string_pretty(&cfg).expect("serialize");
        // None fields should not appear in the serialized output.
        assert!(!toml_str.contains("projects_dir"));
        // Required fields must still be present.
        assert!(toml_str.contains("terraform_dir"));
    }

    #[test]
    fn deserialize_missing_optional_fields() {
        // A TOML document that omits Option<String> fields and cluster_memory_percent entirely.
        let toml_str = r#"
            terraform_dir = "terraform"
            helm_chart_dir = "helm/claude-code"
            claude_mode = "daemon"
            git_user_name = "Claude Code Bot"
            git_user_email = "claude-bot@localhost"
            cpu_limit = "2"
            memory_limit = "4Gi"
        "#;

        let cfg: AppConfig = toml::from_str(toml_str).expect("deserialize");
        assert!(cfg.projects_dir.is_none());
        assert_eq!(cfg.cluster_memory_percent, 80);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("config.toml");

        let cfg = AppConfig {
            projects_dir: Some("/home/user/projects".into()),
            terraform_dir: "custom-tf".into(),
            helm_chart_dir: "custom-helm".into(),
            claude_mode: "batch".into(),
            git_user_name: "Roundtrip Bot".into(),
            git_user_email: "roundtrip@test.com".into(),
            cpu_limit: "8".into(),
            memory_limit: "16Gi".into(),
            cluster_memory_percent: 65,
            extra_mounts: vec!["/data/shared".into()],
            build_timeout_secs: 900,
            deploy_timeout_secs: 120,
            image_retention_hours: 48,
        };

        cfg.save_to(&path).expect("save");
        let loaded = AppConfig::load_from(&path).expect("load");

        assert_eq!(loaded.projects_dir.as_deref(), Some("/home/user/projects"));
        assert_eq!(loaded.terraform_dir, "custom-tf");
        assert_eq!(loaded.helm_chart_dir, "custom-helm");
        assert_eq!(loaded.claude_mode, "batch");
        assert_eq!(loaded.git_user_name, "Roundtrip Bot");
        assert_eq!(loaded.git_user_email, "roundtrip@test.com");
        assert_eq!(loaded.cpu_limit, "8");
        assert_eq!(loaded.memory_limit, "16Gi");
        assert_eq!(loaded.cluster_memory_percent, 65);
        assert_eq!(loaded.extra_mounts, vec!["/data/shared"]);
        assert_eq!(loaded.build_timeout_secs, 900);
        assert_eq!(loaded.deploy_timeout_secs, 120);
    }

    #[test]
    fn load_nonexistent_file_returns_default() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("does_not_exist.toml");

        let cfg = AppConfig::load_from(&path).expect("load from nonexistent");

        // Should be identical to Default.
        let def = AppConfig::default();
        assert_eq!(cfg.projects_dir, def.projects_dir);
        assert_eq!(cfg.terraform_dir, def.terraform_dir);
        assert_eq!(cfg.helm_chart_dir, def.helm_chart_dir);
        assert_eq!(cfg.claude_mode, def.claude_mode);
        assert_eq!(cfg.git_user_name, def.git_user_name);
        assert_eq!(cfg.git_user_email, def.git_user_email);
        assert_eq!(cfg.cpu_limit, def.cpu_limit);
        assert_eq!(cfg.memory_limit, def.memory_limit);
    }

    #[test]
    fn default_cluster_memory_percent() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.cluster_memory_percent, 80);
    }

    #[test]
    fn cluster_memory_percent_roundtrip() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("config.toml");

        let cfg = AppConfig {
            cluster_memory_percent: 70,
            ..AppConfig::default()
        };
        cfg.save_to(&path).expect("save");
        let loaded = AppConfig::load_from(&path).expect("load");
        assert_eq!(loaded.cluster_memory_percent, 70);
    }

    // =========================================================================
    // Corrupt / invalid TOML
    // =========================================================================

    #[test]
    fn load_corrupt_toml_falls_back_to_defaults() {
        // APP-16: Corrupt config should return defaults, not error
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "this is not valid toml {{{{").unwrap();
        let cfg = AppConfig::load_from(&path).expect("should fall back to defaults");
        let def = AppConfig::default();
        assert_eq!(cfg.terraform_dir, def.terraform_dir);
        assert_eq!(cfg.claude_mode, def.claude_mode);
    }

    #[test]
    fn load_empty_file_falls_back_to_defaults() {
        // APP-16: Empty TOML (missing required fields) falls back to defaults
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        let cfg = AppConfig::load_from(&path).expect("should fall back to defaults");
        assert_eq!(cfg.terraform_dir, "terraform");
    }

    #[test]
    fn load_partial_toml_missing_required_fields_falls_back() {
        // APP-16: Partial TOML falls back to defaults
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "terraform_dir = \"tf\"\n").unwrap();
        let cfg = AppConfig::load_from(&path).expect("should fall back to defaults");
        assert_eq!(cfg.terraform_dir, "terraform");
    }

    #[test]
    fn load_wrong_type_for_field_falls_back() {
        // APP-16: Wrong type falls back to defaults
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("config.toml");
        let content = r#"
            terraform_dir = "terraform"
            helm_chart_dir = "helm/claude-code"
            claude_mode = "daemon"
            git_user_name = "Bot"
            git_user_email = "bot@test"
            cpu_limit = "2"
            memory_limit = "4Gi"
            cluster_memory_percent = "not a number"
        "#;
        std::fs::write(&path, content).unwrap();
        let cfg = AppConfig::load_from(&path).expect("should fall back to defaults");
        assert_eq!(cfg.cluster_memory_percent, 80);
    }

    // =========================================================================
    // Unknown / extra fields
    // =========================================================================

    #[test]
    fn load_unknown_fields_are_ignored() {
        // serde by default ignores unknown fields
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("config.toml");
        let content = r#"
            terraform_dir = "terraform"
            helm_chart_dir = "helm/claude-code"
            claude_mode = "daemon"
            git_user_name = "Bot"
            git_user_email = "bot@test"
            cpu_limit = "2"
            memory_limit = "4Gi"
            some_future_field = "hello"
            another_unknown = 42
        "#;
        std::fs::write(&path, content).unwrap();
        let cfg = AppConfig::load_from(&path).expect("should ignore unknown fields");
        assert_eq!(cfg.terraform_dir, "terraform");
    }

    // =========================================================================
    // Empty string projects_dir
    // =========================================================================

    #[test]
    fn empty_string_projects_dir_roundtrips() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("config.toml");
        let cfg = AppConfig {
            projects_dir: Some("".into()),
            ..AppConfig::default()
        };
        cfg.save_to(&path).expect("save");
        let loaded = AppConfig::load_from(&path).expect("load");
        assert_eq!(loaded.projects_dir.as_deref(), Some(""));
    }

    // =========================================================================
    // Path edge cases
    // =========================================================================

    #[test]
    fn projects_dir_with_spaces_and_special_chars() {
        let cfg = AppConfig {
            projects_dir: Some("/home/user/my projects (2)/café".into()),
            ..AppConfig::default()
        };
        let toml_str = toml::to_string_pretty(&cfg).expect("serialize");
        let loaded: AppConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(
            loaded.projects_dir.as_deref(),
            Some("/home/user/my projects (2)/café")
        );
    }

    #[test]
    fn projects_dir_with_backslashes() {
        let cfg = AppConfig {
            projects_dir: Some("C:\\Users\\test\\projects".into()),
            ..AppConfig::default()
        };
        let toml_str = toml::to_string_pretty(&cfg).expect("serialize");
        let loaded: AppConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(
            loaded.projects_dir.as_deref(),
            Some("C:\\Users\\test\\projects")
        );
    }

    // =========================================================================
    // Boundary values for cluster_memory_percent
    // =========================================================================

    #[test]
    fn cluster_memory_percent_zero() {
        let cfg = AppConfig {
            cluster_memory_percent: 0,
            ..AppConfig::default()
        };
        let toml_str = toml::to_string_pretty(&cfg).expect("serialize");
        let loaded: AppConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(loaded.cluster_memory_percent, 0);
    }

    #[test]
    fn cluster_memory_percent_max_u8() {
        let cfg = AppConfig {
            cluster_memory_percent: 255,
            ..AppConfig::default()
        };
        let toml_str = toml::to_string_pretty(&cfg).expect("serialize");
        let loaded: AppConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(loaded.cluster_memory_percent, 255);
    }

    #[test]
    fn cluster_memory_percent_overflow_falls_back() {
        // APP-16: 300 overflows u8, should fall back to defaults
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("config.toml");
        let content = r#"
            terraform_dir = "terraform"
            helm_chart_dir = "helm/claude-code"
            claude_mode = "daemon"
            git_user_name = "Bot"
            git_user_email = "bot@test"
            cpu_limit = "2"
            memory_limit = "4Gi"
            cluster_memory_percent = 300
        "#;
        std::fs::write(&path, content).unwrap();
        let cfg = AppConfig::load_from(&path).expect("should fall back to defaults");
        assert_eq!(cfg.cluster_memory_percent, 80);
    }

    // =========================================================================
    // Save to nested non-existent directory
    // =========================================================================

    #[test]
    fn save_creates_parent_directories() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("a").join("b").join("c").join("config.toml");
        let cfg = AppConfig::default();
        cfg.save_to(&path).expect("save should create parent dirs");
        assert!(path.exists());
    }

    // =========================================================================
    // Unicode in all string fields
    // =========================================================================

    #[test]
    fn unicode_in_all_fields() {
        let cfg = AppConfig {
            projects_dir: Some("日本語/パス".into()),
            terraform_dir: "テラフォーム".into(),
            helm_chart_dir: "ヘルム".into(),
            claude_mode: "モード".into(),
            git_user_name: "クロード".into(),
            git_user_email: "クロード@テスト.jp".into(),
            cpu_limit: "2".into(),
            memory_limit: "4Gi".into(),
            cluster_memory_percent: 80,
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&cfg).expect("serialize");
        let loaded: AppConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(loaded.projects_dir.as_deref(), Some("日本語/パス"));
        assert_eq!(loaded.git_user_name, "クロード");
    }

    // =========================================================================
    // SET-19: Validation
    // =========================================================================

    #[test]
    fn validate_default_config_is_valid() {
        let cfg = AppConfig::default();
        assert!(cfg.validate().is_empty(), "default config should be valid");
    }

    #[test]
    fn validate_empty_cpu_limit() {
        let cfg = AppConfig {
            cpu_limit: "".into(),
            ..AppConfig::default()
        };
        let errors = cfg.validate();
        assert!(errors.iter().any(|e| e.contains("CPU limit")));
    }

    #[test]
    fn validate_non_numeric_cpu_limit() {
        let cfg = AppConfig {
            cpu_limit: "abc".into(),
            ..AppConfig::default()
        };
        let errors = cfg.validate();
        assert!(errors.iter().any(|e| e.contains("CPU limit")));
    }

    #[test]
    fn validate_valid_cpu_limit() {
        let cfg = AppConfig {
            cpu_limit: "4".into(),
            ..AppConfig::default()
        };
        assert!(cfg.validate().is_empty());
    }

    #[test]
    fn validate_fractional_cpu_limit() {
        let cfg = AppConfig {
            cpu_limit: "0.5".into(),
            ..AppConfig::default()
        };
        assert!(cfg.validate().is_empty());
    }

    #[test]
    fn validate_empty_memory_limit() {
        let cfg = AppConfig {
            memory_limit: "".into(),
            ..AppConfig::default()
        };
        let errors = cfg.validate();
        assert!(errors.iter().any(|e| e.contains("Memory limit")));
    }

    #[test]
    fn validate_invalid_memory_limit() {
        let cfg = AppConfig {
            memory_limit: "lots".into(),
            ..AppConfig::default()
        };
        let errors = cfg.validate();
        assert!(errors.iter().any(|e| e.contains("Memory limit")));
    }

    #[test]
    fn validate_valid_memory_gi() {
        let cfg = AppConfig {
            memory_limit: "8Gi".into(),
            ..AppConfig::default()
        };
        assert!(cfg.validate().is_empty());
    }

    #[test]
    fn validate_valid_memory_mi() {
        let cfg = AppConfig {
            memory_limit: "512Mi".into(),
            ..AppConfig::default()
        };
        assert!(cfg.validate().is_empty());
    }

    #[test]
    fn validate_valid_memory_plain_number() {
        let cfg = AppConfig {
            memory_limit: "4096".into(),
            ..AppConfig::default()
        };
        assert!(cfg.validate().is_empty());
    }

    #[test]
    fn validate_empty_git_user_name() {
        let cfg = AppConfig {
            git_user_name: "  ".into(),
            ..AppConfig::default()
        };
        let errors = cfg.validate();
        assert!(errors.iter().any(|e| e.contains("Git user name")));
    }

    #[test]
    fn validate_empty_terraform_dir() {
        let cfg = AppConfig {
            terraform_dir: "".into(),
            ..AppConfig::default()
        };
        let errors = cfg.validate();
        assert!(errors.iter().any(|e| e.contains("Terraform directory")));
    }

    #[test]
    fn validate_empty_helm_chart_dir() {
        let cfg = AppConfig {
            helm_chart_dir: " ".into(),
            ..AppConfig::default()
        };
        let errors = cfg.validate();
        assert!(errors.iter().any(|e| e.contains("Helm chart directory")));
    }

    #[test]
    fn validate_multiple_errors() {
        let cfg = AppConfig {
            cpu_limit: "".into(),
            memory_limit: "bad".into(),
            git_user_name: "".into(),
            ..AppConfig::default()
        };
        let errors = cfg.validate();
        assert!(errors.len() >= 3, "should have at least 3 errors, got: {:?}", errors);
    }

    // =========================================================================
    // PRJ-30: parse_memory_limit_mb
    // =========================================================================

    #[test]
    fn parse_memory_limit_mb_gi() {
        assert_eq!(parse_memory_limit_mb("4Gi"), 4096);
        assert_eq!(parse_memory_limit_mb("8Gi"), 8192);
    }

    #[test]
    fn parse_memory_limit_mb_mi() {
        assert_eq!(parse_memory_limit_mb("512Mi"), 512);
        assert_eq!(parse_memory_limit_mb("2048Mi"), 2048);
    }

    #[test]
    fn parse_memory_limit_mb_plain_number() {
        assert_eq!(parse_memory_limit_mb("4096"), 4096);
    }

    #[test]
    fn parse_memory_limit_mb_invalid() {
        // Falls back to 4096 (default)
        assert_eq!(parse_memory_limit_mb("bad"), 4096);
    }
}
