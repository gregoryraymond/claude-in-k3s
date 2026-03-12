use crate::error::AppResult;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    pub selected: bool,
    pub base_image: BaseImage,
    pub has_custom_dockerfile: bool,
    /// PRJ-61: True when multiple language markers are detected (ambiguous detection).
    pub ambiguous: bool,
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

/// PRJ-61: Detect all language markers present in a project directory.
/// Returns a list of detected languages when multiple are found.
pub fn detect_language_markers(project_path: &Path) -> Vec<BaseImage> {
    let mut markers = Vec::new();

    if project_path.join("package.json").exists() {
        markers.push(BaseImage::Node);
    }
    if project_path.join("Cargo.toml").exists() {
        markers.push(BaseImage::Rust);
    }
    if project_path.join("go.mod").exists() {
        markers.push(BaseImage::Go);
    }
    if project_path.join("requirements.txt").exists()
        || project_path.join("pyproject.toml").exists()
        || project_path.join("setup.py").exists()
    {
        markers.push(BaseImage::Python);
    }
    if project_path.join(".csproj").exists()
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
        markers.push(BaseImage::Dotnet);
    }

    markers
}

/// Returns true if a project has multiple language marker files (ambiguous detection).
pub fn is_ambiguous(project_path: &Path) -> bool {
    detect_language_markers(project_path).len() > 1
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

/// Returns true if the given project path still exists on disk.
pub fn project_dir_exists(project: &Project) -> bool {
    project.path.is_dir()
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

            let ambiguous = !has_custom && is_ambiguous(&path);

            projects.push(Project {
                name,
                path,
                selected: false,
                base_image,
                has_custom_dockerfile: has_custom,
                ambiguous,
            });
        }
    }

    projects.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(projects)
}

/// Quick check: list subdirectory names (non-hidden) in `base_dir`.
/// Returns a sorted list of names for comparison with existing state.
pub fn list_project_names(base_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(base_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names
}

/// Returns `true` if the projects directory has different subdirectories than `current_names`.
pub fn has_projects_changed(base_dir: &Path, current_names: &[String]) -> bool {
    list_project_names(base_dir) != current_names
}

/// PRJ-50: Parse a `.env` file into key-value pairs.
/// Ignores blank lines, comments (#), and malformed lines.
pub fn parse_env_file(path: &Path) -> Vec<(String, String)> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .filter_map(|line| {
            let trimmed = line.trim();
            let pos = trimmed.find('=')?;
            let key = trimmed[..pos].trim().to_string();
            let value = trimmed[pos + 1..].trim().to_string();
            // Strip surrounding quotes from value
            let value = if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                value[1..value.len() - 1].to_string()
            } else {
                value
            };
            if key.is_empty() {
                None
            } else {
                Some((key, value))
            }
        })
        .collect()
}

/// Check if a project has a `.env` file.
pub fn has_env_file(project_path: &Path) -> bool {
    project_path.join(".env").is_file()
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

    // =========================================================================
    // scan_projects edge cases
    // =========================================================================

    #[test]
    fn scan_path_is_a_file_not_directory() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("not_a_dir.txt");
        fs::write(&file_path, "hello").unwrap();
        let projects = scan_projects(&file_path).unwrap();
        assert!(projects.is_empty(), "scanning a file should return empty list");
    }

    #[test]
    fn scan_only_hidden_directories() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join(".git")).unwrap();
        fs::create_dir(tmp.path().join(".vscode")).unwrap();
        fs::create_dir(tmp.path().join(".idea")).unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert!(projects.is_empty(), "only hidden dirs should yield empty list");
    }

    #[test]
    fn scan_only_files_no_directories() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("README.md"), "# Hello").unwrap();
        fs::write(tmp.path().join("LICENSE"), "MIT").unwrap();
        fs::write(tmp.path().join("notes.txt"), "stuff").unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert!(projects.is_empty(), "only files should yield empty list");
    }

    #[test]
    fn scan_mixed_hidden_and_visible() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join(".hidden")).unwrap();
        fs::create_dir(tmp.path().join("visible1")).unwrap();
        fs::create_dir(tmp.path().join(".also_hidden")).unwrap();
        fs::create_dir(tmp.path().join("visible2")).unwrap();
        fs::write(tmp.path().join("file.txt"), "").unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].name, "visible1");
        assert_eq!(projects[1].name, "visible2");
    }

    #[test]
    fn scan_project_name_with_spaces() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("my project")).unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "my project");
    }

    #[test]
    fn scan_project_name_with_special_chars() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("project-v2.0_final")).unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "project-v2.0_final");
    }

    #[test]
    fn scan_project_name_with_unicode() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("项目")).unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "项目");
    }

    #[test]
    fn scan_single_char_project_name() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("a")).unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "a");
    }

    #[test]
    fn scan_projects_all_start_unselected() {
        let tmp = TempDir::new().unwrap();
        for i in 0..5 {
            fs::create_dir(tmp.path().join(format!("proj{}", i))).unwrap();
        }
        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects.len(), 5);
        assert!(projects.iter().all(|p| !p.selected), "all projects should start unselected");
    }

    #[test]
    fn scan_projects_paths_are_absolute() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("myproj")).unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert!(projects[0].path.is_absolute(), "project path should be absolute");
        assert!(projects[0].path.ends_with("myproj"), "path should end with project name");
    }

    // =========================================================================
    // has_dockerfile edge cases
    // =========================================================================

    #[test]
    fn has_dockerfile_empty_file() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Dockerfile"), "").unwrap();
        assert!(has_dockerfile(tmp.path()), "empty Dockerfile should still be detected");
    }

    #[test]
    fn has_dockerfile_claude_dir_is_file() {
        let tmp = TempDir::new().unwrap();
        // .claude is a file, not a directory
        fs::write(tmp.path().join(".claude"), "not a directory").unwrap();
        assert!(!has_dockerfile(tmp.path()), ".claude as file should not detect Dockerfile");
    }

    #[test]
    fn has_dockerfile_both_locations() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Dockerfile"), "FROM node").unwrap();
        let claude_dir = tmp.path().join(".claude");
        fs::create_dir(&claude_dir).unwrap();
        fs::write(claude_dir.join("Dockerfile"), "FROM python").unwrap();
        assert!(has_dockerfile(tmp.path()), "both locations should still return true");
    }

    // =========================================================================
    // detect_base_image edge cases
    // =========================================================================

    #[test]
    fn detect_rust_priority_over_go() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
        fs::write(tmp.path().join("go.mod"), "").unwrap();
        assert_eq!(detect_base_image(tmp.path()), BaseImage::Rust, "Rust should take priority over Go");
    }

    #[test]
    fn detect_go_priority_over_python() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("go.mod"), "").unwrap();
        fs::write(tmp.path().join("requirements.txt"), "").unwrap();
        assert_eq!(detect_base_image(tmp.path()), BaseImage::Go, "Go should take priority over Python");
    }

    #[test]
    fn detect_python_priority_over_dotnet() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("requirements.txt"), "").unwrap();
        fs::write(tmp.path().join("MyApp.csproj"), "").unwrap();
        assert_eq!(detect_base_image(tmp.path()), BaseImage::Python, "Python should take priority over Dotnet");
    }

    #[test]
    fn detect_nonexistent_path_returns_base() {
        let path = Path::new("/tmp/definitely_does_not_exist_12345");
        assert_eq!(detect_base_image(path), BaseImage::Base, "nonexistent path should default to Base");
    }

    #[test]
    fn detect_dotnet_from_extension_scan() {
        // Test .csproj detection via directory enumeration (not exact filename match)
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("MyApp.csproj"), "<Project/>").unwrap();
        assert_eq!(detect_base_image(tmp.path()), BaseImage::Dotnet);
    }

    #[test]
    fn detect_dotnet_from_sln_extension() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Solution.sln"), "").unwrap();
        assert_eq!(detect_base_image(tmp.path()), BaseImage::Dotnet);
    }

    // =========================================================================
    // BaseImage index edge cases
    // =========================================================================

    #[test]
    fn from_index_negative_values() {
        assert_eq!(BaseImage::from_index(-100), BaseImage::Node);
        assert_eq!(BaseImage::from_index(i32::MIN), BaseImage::Node);
    }

    #[test]
    fn from_index_large_values() {
        assert_eq!(BaseImage::from_index(i32::MAX), BaseImage::Node);
        assert_eq!(BaseImage::from_index(1000), BaseImage::Node);
    }

    #[test]
    fn to_index_from_index_roundtrip_all() {
        let all = vec![
            BaseImage::Node, BaseImage::Python, BaseImage::Rust,
            BaseImage::Go, BaseImage::Dotnet, BaseImage::Base, BaseImage::Custom,
        ];
        for img in all {
            let idx = img.to_index();
            let back = BaseImage::from_index(idx);
            assert_eq!(back, img, "roundtrip failed for {:?} (index {})", img, idx);
        }
    }

    // =========================================================================
    // Custom Dockerfile overrides base image detection
    // =========================================================================

    #[test]
    fn scan_custom_dockerfile_overrides_node_detection() {
        let tmp = TempDir::new().unwrap();
        let proj = tmp.path().join("myproj");
        fs::create_dir(&proj).unwrap();
        fs::write(proj.join("package.json"), "{}").unwrap();
        fs::write(proj.join("Dockerfile"), "FROM custom").unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects[0].base_image, BaseImage::Custom, "Dockerfile should override Node detection");
        assert!(projects[0].has_custom_dockerfile);
    }

    #[test]
    fn scan_claude_dockerfile_overrides_detection() {
        let tmp = TempDir::new().unwrap();
        let proj = tmp.path().join("myproj");
        fs::create_dir(&proj).unwrap();
        fs::write(proj.join("Cargo.toml"), "").unwrap();
        let claude_dir = proj.join(".claude");
        fs::create_dir(&claude_dir).unwrap();
        fs::write(claude_dir.join("Dockerfile"), "FROM custom").unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects[0].base_image, BaseImage::Custom, ".claude/Dockerfile should override Rust detection");
        assert!(projects[0].has_custom_dockerfile);
    }

    // =========================================================================
    // Large directory scan
    // =========================================================================

    #[test]
    fn scan_many_projects() {
        let tmp = TempDir::new().unwrap();
        for i in 0..50 {
            fs::create_dir(tmp.path().join(format!("project-{:03}", i))).unwrap();
        }
        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects.len(), 50);
        // Verify sorted
        for i in 1..projects.len() {
            assert!(projects[i - 1].name < projects[i].name, "should be sorted: {} < {}", projects[i-1].name, projects[i].name);
        }
    }

    // =========================================================================
    // PRJ-43/PRJ-58: Filesystem change detection (polling)
    // =========================================================================

    #[test]
    fn list_project_names_empty_dir() {
        let tmp = TempDir::new().unwrap();
        assert!(list_project_names(tmp.path()).is_empty());
    }

    #[test]
    fn list_project_names_sorted() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("charlie")).unwrap();
        fs::create_dir(tmp.path().join("alpha")).unwrap();
        fs::create_dir(tmp.path().join("bravo")).unwrap();
        let names = list_project_names(tmp.path());
        assert_eq!(names, vec!["alpha", "bravo", "charlie"]);
    }

    #[test]
    fn list_project_names_skips_hidden() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join(".hidden")).unwrap();
        fs::create_dir(tmp.path().join("visible")).unwrap();
        let names = list_project_names(tmp.path());
        assert_eq!(names, vec!["visible"]);
    }

    #[test]
    fn has_projects_changed_no_change() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("aaa")).unwrap();
        fs::create_dir(tmp.path().join("bbb")).unwrap();
        let current = vec!["aaa".to_string(), "bbb".to_string()];
        assert!(!has_projects_changed(tmp.path(), &current));
    }

    #[test]
    fn has_projects_changed_added() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("aaa")).unwrap();
        fs::create_dir(tmp.path().join("bbb")).unwrap();
        fs::create_dir(tmp.path().join("ccc")).unwrap();
        let current = vec!["aaa".to_string(), "bbb".to_string()];
        assert!(has_projects_changed(tmp.path(), &current));
    }

    #[test]
    fn has_projects_changed_removed() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("aaa")).unwrap();
        let current = vec!["aaa".to_string(), "bbb".to_string()];
        assert!(has_projects_changed(tmp.path(), &current));
    }

    #[test]
    fn has_projects_changed_nonexistent_dir() {
        let current = vec!["aaa".to_string()];
        assert!(has_projects_changed(Path::new("/nonexistent/dir"), &current));
    }

    // =========================================================================
    // PRJ-50: .env file parsing
    // =========================================================================

    #[test]
    fn parse_env_file_basic() {
        let tmp = TempDir::new().unwrap();
        let env_path = tmp.path().join(".env");
        fs::write(&env_path, "KEY1=value1\nKEY2=value2\n").unwrap();
        let vars = parse_env_file(&env_path);
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0], ("KEY1".to_string(), "value1".to_string()));
        assert_eq!(vars[1], ("KEY2".to_string(), "value2".to_string()));
    }

    #[test]
    fn parse_env_file_ignores_comments() {
        let tmp = TempDir::new().unwrap();
        let env_path = tmp.path().join(".env");
        fs::write(&env_path, "# comment\nKEY=val\n# another\n").unwrap();
        let vars = parse_env_file(&env_path);
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].0, "KEY");
    }

    #[test]
    fn parse_env_file_ignores_blank_lines() {
        let tmp = TempDir::new().unwrap();
        let env_path = tmp.path().join(".env");
        fs::write(&env_path, "\n\nKEY=val\n\n").unwrap();
        let vars = parse_env_file(&env_path);
        assert_eq!(vars.len(), 1);
    }

    #[test]
    fn parse_env_file_strips_quotes() {
        let tmp = TempDir::new().unwrap();
        let env_path = tmp.path().join(".env");
        fs::write(&env_path, "A=\"quoted\"\nB='single'\nC=plain\n").unwrap();
        let vars = parse_env_file(&env_path);
        assert_eq!(vars.len(), 3);
        assert_eq!(vars[0], ("A".to_string(), "quoted".to_string()));
        assert_eq!(vars[1], ("B".to_string(), "single".to_string()));
        assert_eq!(vars[2], ("C".to_string(), "plain".to_string()));
    }

    #[test]
    fn parse_env_file_handles_equals_in_value() {
        let tmp = TempDir::new().unwrap();
        let env_path = tmp.path().join(".env");
        fs::write(&env_path, "DB_URL=postgres://host?opt=1\n").unwrap();
        let vars = parse_env_file(&env_path);
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].0, "DB_URL");
        assert_eq!(vars[0].1, "postgres://host?opt=1");
    }

    #[test]
    fn parse_env_file_nonexistent_returns_empty() {
        let vars = parse_env_file(Path::new("/no/such/.env"));
        assert!(vars.is_empty());
    }

    #[test]
    fn parse_env_file_empty_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let env_path = tmp.path().join(".env");
        fs::write(&env_path, "").unwrap();
        let vars = parse_env_file(&env_path);
        assert!(vars.is_empty());
    }

    #[test]
    fn parse_env_file_skips_empty_key() {
        let tmp = TempDir::new().unwrap();
        let env_path = tmp.path().join(".env");
        fs::write(&env_path, "=value\nGOOD=ok\n").unwrap();
        let vars = parse_env_file(&env_path);
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].0, "GOOD");
    }

    #[test]
    fn has_env_file_true() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".env"), "KEY=val").unwrap();
        assert!(has_env_file(tmp.path()));
    }

    #[test]
    fn has_env_file_false() {
        let tmp = TempDir::new().unwrap();
        assert!(!has_env_file(tmp.path()));
    }

    // PRJ-61: Language marker detection tests
    #[test]
    fn detect_markers_single_node() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        let markers = detect_language_markers(tmp.path());
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], BaseImage::Node);
        assert!(!is_ambiguous(tmp.path()));
    }

    #[test]
    fn detect_markers_multiple_languages() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        fs::write(tmp.path().join("requirements.txt"), "flask").unwrap();
        let markers = detect_language_markers(tmp.path());
        assert_eq!(markers.len(), 2);
        assert!(markers.contains(&BaseImage::Node));
        assert!(markers.contains(&BaseImage::Python));
        assert!(is_ambiguous(tmp.path()));
    }

    #[test]
    fn detect_markers_no_languages() {
        let tmp = TempDir::new().unwrap();
        let markers = detect_language_markers(tmp.path());
        assert!(markers.is_empty());
        assert!(!is_ambiguous(tmp.path()));
    }

    #[test]
    fn detect_markers_three_languages() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "[package]").unwrap();
        fs::write(tmp.path().join("go.mod"), "module test").unwrap();
        let markers = detect_language_markers(tmp.path());
        assert_eq!(markers.len(), 3);
        assert!(is_ambiguous(tmp.path()));
    }

    #[test]
    fn ambiguous_flag_set_in_scan() {
        let tmp = TempDir::new().unwrap();
        let proj_dir = tmp.path().join("multi-lang-proj");
        fs::create_dir(&proj_dir).unwrap();
        fs::write(proj_dir.join("package.json"), "{}").unwrap();
        fs::write(proj_dir.join("requirements.txt"), "flask").unwrap();
        let projects = scan_projects(tmp.path()).unwrap();
        assert_eq!(projects.len(), 1);
        assert!(projects[0].ambiguous, "project with multiple language markers should be ambiguous");
    }
}
