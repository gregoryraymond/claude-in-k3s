use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub projects_dir: Option<String>,
    pub api_key: Option<String>,
    pub terraform_dir: String,
    pub helm_chart_dir: String,
    pub claude_mode: String,
    pub git_user_name: String,
    pub git_user_email: String,
    pub cpu_limit: String,
    pub memory_limit: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            projects_dir: None,
            api_key: None,
            terraform_dir: "terraform".into(),
            helm_chart_dir: "helm/claude-code".into(),
            claude_mode: "daemon".into(),
            git_user_name: "Claude Code Bot".into(),
            git_user_email: "claude-bot@localhost".into(),
            cpu_limit: "2".into(),
            memory_limit: "4Gi".into(),
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
            toml::from_str(&content).map_err(|e| AppError::Config(e.to_string()))
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

    /// Load config from an arbitrary path (returns default if file does not exist).
    #[cfg(test)]
    fn load_from(path: &std::path::Path) -> AppResult<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            toml::from_str(&content).map_err(|e| AppError::Config(e.to_string()))
        } else {
            Ok(Self::default())
        }
    }

    /// Save config to an arbitrary path.
    #[cfg(test)]
    fn save_to(&self, path: &std::path::Path) -> AppResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content =
            toml::to_string_pretty(self).map_err(|e| AppError::Config(e.to_string()))?;
        std::fs::write(path, content)?;
        Ok(())
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
        assert!(cfg.api_key.is_none());
        assert_eq!(cfg.terraform_dir, "terraform");
        assert_eq!(cfg.helm_chart_dir, "helm/claude-code");
        assert_eq!(cfg.claude_mode, "daemon");
        assert_eq!(cfg.git_user_name, "Claude Code Bot");
        assert_eq!(cfg.git_user_email, "claude-bot@localhost");
        assert_eq!(cfg.cpu_limit, "2");
        assert_eq!(cfg.memory_limit, "4Gi");
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
            api_key: Some("sk-test-key".into()),
            terraform_dir: "my-tf".into(),
            helm_chart_dir: "charts/claude".into(),
            claude_mode: "interactive".into(),
            git_user_name: "Test User".into(),
            git_user_email: "test@example.com".into(),
            cpu_limit: "4".into(),
            memory_limit: "8Gi".into(),
        };

        let toml_str = toml::to_string_pretty(&cfg).expect("serialize");
        let loaded: AppConfig = toml::from_str(&toml_str).expect("deserialize");

        assert_eq!(loaded.projects_dir.as_deref(), Some("/tmp/projects"));
        assert_eq!(loaded.api_key.as_deref(), Some("sk-test-key"));
        assert_eq!(loaded.terraform_dir, "my-tf");
        assert_eq!(loaded.helm_chart_dir, "charts/claude");
        assert_eq!(loaded.claude_mode, "interactive");
        assert_eq!(loaded.git_user_name, "Test User");
        assert_eq!(loaded.git_user_email, "test@example.com");
        assert_eq!(loaded.cpu_limit, "4");
        assert_eq!(loaded.memory_limit, "8Gi");
    }

    #[test]
    fn serialize_with_none_fields() {
        let cfg = AppConfig {
            projects_dir: None,
            api_key: None,
            ..AppConfig::default()
        };

        let toml_str = toml::to_string_pretty(&cfg).expect("serialize");
        // None fields should not appear in the serialized output.
        assert!(!toml_str.contains("projects_dir"));
        assert!(!toml_str.contains("api_key"));
        // Required fields must still be present.
        assert!(toml_str.contains("terraform_dir"));
    }

    #[test]
    fn deserialize_missing_optional_fields() {
        // A TOML document that omits the two Option<String> fields entirely.
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
        assert!(cfg.api_key.is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("config.toml");

        let cfg = AppConfig {
            projects_dir: Some("/home/user/projects".into()),
            api_key: Some("sk-roundtrip".into()),
            terraform_dir: "custom-tf".into(),
            helm_chart_dir: "custom-helm".into(),
            claude_mode: "batch".into(),
            git_user_name: "Roundtrip Bot".into(),
            git_user_email: "roundtrip@test.com".into(),
            cpu_limit: "8".into(),
            memory_limit: "16Gi".into(),
        };

        cfg.save_to(&path).expect("save");
        let loaded = AppConfig::load_from(&path).expect("load");

        assert_eq!(loaded.projects_dir.as_deref(), Some("/home/user/projects"));
        assert_eq!(loaded.api_key.as_deref(), Some("sk-roundtrip"));
        assert_eq!(loaded.terraform_dir, "custom-tf");
        assert_eq!(loaded.helm_chart_dir, "custom-helm");
        assert_eq!(loaded.claude_mode, "batch");
        assert_eq!(loaded.git_user_name, "Roundtrip Bot");
        assert_eq!(loaded.git_user_email, "roundtrip@test.com");
        assert_eq!(loaded.cpu_limit, "8");
        assert_eq!(loaded.memory_limit, "16Gi");
    }

    #[test]
    fn load_nonexistent_file_returns_default() {
        let tmp = TempDir::new().expect("create temp dir");
        let path = tmp.path().join("does_not_exist.toml");

        let cfg = AppConfig::load_from(&path).expect("load from nonexistent");

        // Should be identical to Default.
        let def = AppConfig::default();
        assert_eq!(cfg.projects_dir, def.projects_dir);
        assert_eq!(cfg.api_key, def.api_key);
        assert_eq!(cfg.terraform_dir, def.terraform_dir);
        assert_eq!(cfg.helm_chart_dir, def.helm_chart_dir);
        assert_eq!(cfg.claude_mode, def.claude_mode);
        assert_eq!(cfg.git_user_name, def.git_user_name);
        assert_eq!(cfg.git_user_email, def.git_user_email);
        assert_eq!(cfg.cpu_limit, def.cpu_limit);
        assert_eq!(cfg.memory_limit, def.memory_limit);
    }
}
