use crate::error::AppResult;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    pub selected: bool,
    pub base_image: BaseImage,
    pub has_custom_dockerfile: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BaseImage {
    Node,
    Python,
    Rust,
    Go,
    Dotnet,
    Base,
    Custom,
}

impl BaseImage {
    pub fn docker_image(&self) -> &'static str {
        match self {
            Self::Node => "node:22-bookworm-slim",
            Self::Python => "python:3.12-slim-bookworm",
            Self::Rust => "rust:1.83-slim-bookworm",
            Self::Go => "golang:1.23-bookworm",
            Self::Dotnet => "mcr.microsoft.com/dotnet/sdk:9.0",
            Self::Base => "debian:bookworm-slim",
            Self::Custom => "custom",
        }
    }

    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Node => "Node.js",
            Self::Python => "Python",
            Self::Rust => "Rust",
            Self::Go => "Go",
            Self::Dotnet => ".NET",
            Self::Base => "Minimal",
            Self::Custom => "Custom",
        }
    }

    #[allow(dead_code)]
    pub fn all_presets() -> &'static [BaseImage] {
        &[
            Self::Node,
            Self::Python,
            Self::Rust,
            Self::Go,
            Self::Dotnet,
            Self::Base,
        ]
    }

    pub fn from_index(idx: i32) -> Self {
        match idx {
            0 => Self::Node,
            1 => Self::Python,
            2 => Self::Rust,
            3 => Self::Go,
            4 => Self::Dotnet,
            5 => Self::Base,
            6 => Self::Custom,
            _ => Self::Node,
        }
    }

    pub fn to_index(&self) -> i32 {
        match self {
            Self::Node => 0,
            Self::Python => 1,
            Self::Rust => 2,
            Self::Go => 3,
            Self::Dotnet => 4,
            Self::Base => 5,
            Self::Custom => 6,
        }
    }
}

/// Detect if a project has a custom Dockerfile
fn has_dockerfile(project_path: &Path) -> bool {
    project_path.join("Dockerfile").exists()
        || project_path.join(".claude").join("Dockerfile").exists()
}

/// Auto-detect the best base image for a project based on its files
fn detect_base_image(project_path: &Path) -> BaseImage {
    if project_path.join("package.json").exists() {
        BaseImage::Node
    } else if project_path.join("Cargo.toml").exists() {
        BaseImage::Rust
    } else if project_path.join("go.mod").exists() {
        BaseImage::Go
    } else if project_path.join("requirements.txt").exists()
        || project_path.join("pyproject.toml").exists()
        || project_path.join("setup.py").exists()
    {
        BaseImage::Python
    } else if project_path.join(".csproj").exists()
        || project_path.join(".sln").exists()
        || std::fs::read_dir(project_path)
            .ok()
            .map(|entries| {
                entries.filter_map(|e| e.ok()).any(|e| {
                    e.path()
                        .extension()
                        .map(|ext| ext == "csproj" || ext == "sln")
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    {
        BaseImage::Dotnet
    } else {
        BaseImage::Base
    }
}

/// Scan a directory for project subdirectories
pub fn scan_projects(base_dir: &Path) -> AppResult<Vec<Project>> {
    let mut projects = Vec::new();

    if !base_dir.exists() || !base_dir.is_dir() {
        return Ok(projects);
    }

    let entries = std::fs::read_dir(base_dir)?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }

            let has_custom = has_dockerfile(&path);
            let base_image = if has_custom {
                BaseImage::Custom
            } else {
                detect_base_image(&path)
            };

            projects.push(Project {
                name,
                path,
                selected: false,
                base_image,
                has_custom_dockerfile: has_custom,
            });
        }
    }

    projects.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(projects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    // ---- BaseImage::docker_image() ----

    #[test]
    fn base_image_docker_images() {
        assert_eq!(BaseImage::Node.docker_image(), "node:22-bookworm-slim");
        assert_eq!(BaseImage::Python.docker_image(), "python:3.12-slim-bookworm");
        assert_eq!(BaseImage::Rust.docker_image(), "rust:1.83-slim-bookworm");
        assert_eq!(BaseImage::Go.docker_image(), "golang:1.23-bookworm");
        assert_eq!(BaseImage::Dotnet.docker_image(), "mcr.microsoft.com/dotnet/sdk:9.0");
        assert_eq!(BaseImage::Base.docker_image(), "debian:bookworm-slim");
        assert_eq!(BaseImage::Custom.docker_image(), "custom");
    }

    // ---- BaseImage::label() ----

    #[test]
    fn base_image_labels() {
        assert_eq!(BaseImage::Node.label(), "Node.js");
        assert_eq!(BaseImage::Python.label(), "Python");
        assert_eq!(BaseImage::Rust.label(), "Rust");
        assert_eq!(BaseImage::Go.label(), "Go");
        assert_eq!(BaseImage::Dotnet.label(), ".NET");
        assert_eq!(BaseImage::Base.label(), "Minimal");
        assert_eq!(BaseImage::Custom.label(), "Custom");
    }

    // ---- BaseImage::all_presets() ----

    #[test]
    fn all_presets_excludes_custom() {
        let presets = BaseImage::all_presets();
        assert_eq!(presets.len(), 6);
        assert!(!presets.contains(&BaseImage::Custom));
        assert!(presets.contains(&BaseImage::Node));
        assert!(presets.contains(&BaseImage::Python));
        assert!(presets.contains(&BaseImage::Rust));
        assert!(presets.contains(&BaseImage::Go));
        assert!(presets.contains(&BaseImage::Dotnet));
        assert!(presets.contains(&BaseImage::Base));
    }

    // ---- BaseImage::from_index() ----

    #[test]
    fn from_index_valid_values() {
        assert_eq!(BaseImage::from_index(0), BaseImage::Node);
        assert_eq!(BaseImage::from_index(1), BaseImage::Python);
        assert_eq!(BaseImage::from_index(2), BaseImage::Rust);
        assert_eq!(BaseImage::from_index(3), BaseImage::Go);
        assert_eq!(BaseImage::from_index(4), BaseImage::Dotnet);
        assert_eq!(BaseImage::from_index(5), BaseImage::Base);
        assert_eq!(BaseImage::from_index(6), BaseImage::Custom);
    }

    #[test]
    fn from_index_out_of_range_defaults_to_node() {
        assert_eq!(BaseImage::from_index(7), BaseImage::Node);
        assert_eq!(BaseImage::from_index(-1), BaseImage::Node);
        assert_eq!(BaseImage::from_index(100), BaseImage::Node);
    }

    // ---- BaseImage::to_index() ----

    #[test]
    fn to_index_values() {
        assert_eq!(BaseImage::Node.to_index(), 0);
        assert_eq!(BaseImage::Python.to_index(), 1);
        assert_eq!(BaseImage::Rust.to_index(), 2);
        assert_eq!(BaseImage::Go.to_index(), 3);
        assert_eq!(BaseImage::Dotnet.to_index(), 4);
        assert_eq!(BaseImage::Base.to_index(), 5);
        assert_eq!(BaseImage::Custom.to_index(), 6);
    }

    // ---- roundtrip ----

    #[test]
    fn from_index_to_index_roundtrip() {
        for idx in 0..=6 {
            let image = BaseImage::from_index(idx);
            assert_eq!(image.to_index(), idx);
        }
    }

    // ---- detect_base_image() ----

    #[test]
    fn detect_node_project() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        assert_eq!(detect_base_image(tmp.path()), BaseImage::Node);
    }

    #[test]
    fn detect_rust_project() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
        assert_eq!(detect_base_image(tmp.path()), BaseImage::Rust);
    }

    #[test]
    fn detect_go_project() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("go.mod"), "").unwrap();
        assert_eq!(detect_base_image(tmp.path()), BaseImage::Go);
    }

    #[test]
    fn detect_python_requirements() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("requirements.txt"), "").unwrap();
        assert_eq!(detect_base_image(tmp.path()), BaseImage::Python);
    }

    #[test]
    fn detect_python_pyproject() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("pyproject.toml"), "").unwrap();
        assert_eq!(detect_base_image(tmp.path()), BaseImage::Python);
    }

    #[test]
    fn detect_python_setup_py() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("setup.py"), "").unwrap();
        assert_eq!(detect_base_image(tmp.path()), BaseImage::Python);
    }

    #[test]
    fn detect_dotnet_csproj() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("MyApp.csproj"), "").unwrap();
        assert_eq!(detect_base_image(tmp.path()), BaseImage::Dotnet);
    }

    #[test]
    fn detect_dotnet_sln() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("MyApp.sln"), "").unwrap();
        assert_eq!(detect_base_image(tmp.path()), BaseImage::Dotnet);
    }

    #[test]
    fn detect_base_fallback() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(detect_base_image(tmp.path()), BaseImage::Base);
    }

    #[test]
    fn detect_node_takes_priority_over_others() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        fs::write(tmp.path().join("requirements.txt"), "").unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
        assert_eq!(detect_base_image(tmp.path()), BaseImage::Node);
    }

    // ---- has_dockerfile() ----

    #[test]
    fn has_dockerfile_root() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Dockerfile"), "FROM debian").unwrap();
        assert!(has_dockerfile(tmp.path()));
    }

    #[test]
    fn has_dockerfile_in_claude_dir() {
        let tmp = TempDir::new().unwrap();
        let claude_dir = tmp.path().join(".claude");
        fs::create_dir(&claude_dir).unwrap();
        fs::write(claude_dir.join("Dockerfile"), "FROM debian").unwrap();
        assert!(has_dockerfile(tmp.path()));
    }

    #[test]
    fn has_no_dockerfile() {
        let tmp = TempDir::new().unwrap();
        assert!(!has_dockerfile(tmp.path()));
    }

    // ---- scan_projects() ----

    #[test]
    fn scan_empty_directory() {
        let tmp = TempDir::new().unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert!(projects.is_empty());
    }

    #[test]
    fn scan_nonexistent_directory() {
        let path = Path::new("/tmp/nonexistent_dir_for_test_12345");
        let projects = scan_projects(path).unwrap();
        assert!(projects.is_empty());
    }

    #[test]
    fn scan_skips_hidden_directories() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join(".hidden")).unwrap();
        fs::create_dir(tmp.path().join("visible")).unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "visible");
    }

    #[test]
    fn scan_skips_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("not_a_dir.txt"), "hello").unwrap();
        fs::create_dir(tmp.path().join("actual_project")).unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "actual_project");
    }

    #[test]
    fn scan_detects_base_image() {
        let tmp = TempDir::new().unwrap();
        let proj = tmp.path().join("my_node_app");
        fs::create_dir(&proj).unwrap();
        fs::write(proj.join("package.json"), "{}").unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].base_image, BaseImage::Node);
        assert!(!projects[0].has_custom_dockerfile);
    }

    #[test]
    fn scan_detects_custom_dockerfile() {
        let tmp = TempDir::new().unwrap();
        let proj = tmp.path().join("custom_proj");
        fs::create_dir(&proj).unwrap();
        fs::write(proj.join("Dockerfile"), "FROM ubuntu").unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].base_image, BaseImage::Custom);
        assert!(projects[0].has_custom_dockerfile);
    }

    #[test]
    fn scan_projects_sorted_alphabetically() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("zebra")).unwrap();
        fs::create_dir(tmp.path().join("alpha")).unwrap();
        fs::create_dir(tmp.path().join("middle")).unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects.len(), 3);
        assert_eq!(projects[0].name, "alpha");
        assert_eq!(projects[1].name, "middle");
        assert_eq!(projects[2].name, "zebra");
    }

    #[test]
    fn scan_projects_default_not_selected() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("proj")).unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert!(!projects[0].selected);
    }

    #[test]
    fn scan_multiple_projects_with_detection() {
        let tmp = TempDir::new().unwrap();

        // Node project
        let node_proj = tmp.path().join("frontend");
        fs::create_dir(&node_proj).unwrap();
        fs::write(node_proj.join("package.json"), "{}").unwrap();

        // Rust project
        let rust_proj = tmp.path().join("backend");
        fs::create_dir(&rust_proj).unwrap();
        fs::write(rust_proj.join("Cargo.toml"), "").unwrap();

        // Python project
        let py_proj = tmp.path().join("scripts");
        fs::create_dir(&py_proj).unwrap();
        fs::write(py_proj.join("requirements.txt"), "").unwrap();

        // Custom dockerfile project
        let custom_proj = tmp.path().join("infra");
        fs::create_dir(&custom_proj).unwrap();
        fs::write(custom_proj.join("Dockerfile"), "FROM alpine").unwrap();

        // Empty project (should be Base)
        let empty_proj = tmp.path().join("docs");
        fs::create_dir(&empty_proj).unwrap();

        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects.len(), 5);

        // Sorted alphabetically: backend, docs, frontend, infra, scripts
        assert_eq!(projects[0].name, "backend");
        assert_eq!(projects[0].base_image, BaseImage::Rust);
        assert!(!projects[0].has_custom_dockerfile);

        assert_eq!(projects[1].name, "docs");
        assert_eq!(projects[1].base_image, BaseImage::Base);
        assert!(!projects[1].has_custom_dockerfile);

        assert_eq!(projects[2].name, "frontend");
        assert_eq!(projects[2].base_image, BaseImage::Node);
        assert!(!projects[2].has_custom_dockerfile);

        assert_eq!(projects[3].name, "infra");
        assert_eq!(projects[3].base_image, BaseImage::Custom);
        assert!(projects[3].has_custom_dockerfile);

        assert_eq!(projects[4].name, "scripts");
        assert_eq!(projects[4].base_image, BaseImage::Python);
        assert!(!projects[4].has_custom_dockerfile);
    }
}
