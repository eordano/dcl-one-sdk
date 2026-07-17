use crate::scene::Project;
use crate::ux::{TrySteps, UserError};
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const WORKSPACE_FILE: &str = "dcl-workspace.json";

#[derive(Debug)]
pub struct Workspace {
    pub root: PathBuf,
    pub projects: Vec<Project>,
}

pub fn file_in(dir: &Path) -> PathBuf {
    dir.join(WORKSPACE_FILE)
}

pub fn member_folders(dir: &Path) -> Result<Option<Vec<String>>> {
    let path = file_in(dir);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let json: Value = serde_json::from_slice(&bytes).map_err(|e| {
        UserError::new(
            format!(
                "{WORKSPACE_FILE} is not valid JSON (line {}, column {})",
                e.line(),
                e.column()
            ),
            TrySteps::one(format!(
                "fix the syntax error at {WORKSPACE_FILE}:{}:{}",
                e.line(),
                e.column()
            ))
            .and("validate the file with a JSON linter"),
        )
        .caused_by(e)
    })?;
    let folders = json
        .get("folders")
        .and_then(|f| f.as_array())
        .ok_or_else(|| shape_error(&path))?;
    if folders.is_empty() {
        return Err(shape_error(&path).into());
    }
    let mut out = Vec::new();
    for entry in folders {
        let Some(p) = entry
            .get("path")
            .and_then(|p| p.as_str())
            .filter(|p| !p.trim().is_empty())
        else {
            return Err(shape_error(&path).into());
        };
        out.push(p.to_string());
    }
    Ok(Some(out))
}

fn shape_error(path: &Path) -> UserError {
    UserError::new(
        format!("{WORKSPACE_FILE} must list at least one folder"),
        TrySteps::one(
            r#"shape it like: { "folders": [ { "path": "scene-a" }, { "path": "scene-b" } ] }"#,
        )
        .and("every entry needs a \"path\" string pointing at a scene folder"),
    )
    .why(format!(
        "{} has no usable \"folders\" array",
        path.display()
    ))
}

impl Workspace {
    pub fn load(dir: &Path) -> Result<Self> {
        match member_folders(dir)? {
            None => {
                let project = Project::load(dir)?;
                let root = project.root.clone();
                Ok(Self {
                    root,
                    projects: vec![project],
                })
            }
            Some(folders) => {
                let root = dunce::canonicalize(dir)
                    .with_context(|| format!("resolving workspace dir {}", dir.display()))?;
                let mut projects = Vec::new();
                for folder in &folders {
                    let project = Project::load(&root.join(folder)).map_err(|e| {
                        anyhow::Error::from(
                            crate::ux::UserError::new(
                                format!(
                                    "workspace member \"{folder}\" (from dcl-workspace.json) failed to load: {}",
                                    crate::ux::concise_cause(&e)
                                ),
                                crate::ux::TrySteps::one(format!(
                                    "check the \"folders\" entry \"{folder}\" in dcl-workspace.json points at a scene directory"
                                ))
                                .and("remove the entry if the scene no longer exists"),
                            )
                            .why(format!("{e:#}")),
                        )
                    })?;
                    projects.push(project);
                }
                Ok(Self { root, projects })
            }
        }
    }

    pub fn is_multi(&self) -> bool {
        self.projects.len() > 1
    }

    pub fn member_header(&self, index: usize) -> Option<String> {
        if !self.is_multi() {
            return None;
        }
        let project = &self.projects[index];
        let rel = project
            .root
            .strip_prefix(&self.root)
            .unwrap_or(&project.root);
        let shown = if rel.as_os_str().is_empty() {
            ".".to_string()
        } else {
            rel.display().to_string()
        };
        Some(format!(
            "[{}/{}] in {shown}:",
            index + 1,
            self.projects.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENE_OK: &str =
        r#"{"main":"bin/index.js","runtimeVersion":"7","scene":{"parcels":["0,0"],"base":"0,0"}}"#;

    struct Tmp(PathBuf);

    impl Tmp {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "dcl-one-sdk-workspace-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Tmp(dir)
        }

        fn write(&self, rel: &str, contents: &str) {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, contents).unwrap();
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn no_workspace_file_loads_a_single_project() {
        let t = Tmp::new("single");
        t.write("scene.json", SCENE_OK);
        assert!(member_folders(&t.0).unwrap().is_none());
        let ws = Workspace::load(&t.0).unwrap();
        assert_eq!(ws.projects.len(), 1);
        assert!(!ws.is_multi());
        assert_eq!(ws.member_header(0), None);
        assert_eq!(ws.root, t.0.canonicalize().unwrap());
    }

    #[test]
    fn two_member_workspace_loads_in_folder_order() {
        let t = Tmp::new("two");
        t.write(
            "dcl-workspace.json",
            r#"{"folders":[{"path":"scene-a"},{"path":"scene-b"}]}"#,
        );
        t.write("scene-a/scene.json", SCENE_OK);
        t.write(
            "scene-b/scene.json",
            r#"{"main":"bin/index.js","runtimeVersion":"7","scene":{"parcels":["1,0"],"base":"1,0"}}"#,
        );
        let ws = Workspace::load(&t.0).unwrap();
        assert!(ws.is_multi());
        assert_eq!(ws.projects.len(), 2);
        let root = t.0.canonicalize().unwrap();
        assert_eq!(ws.projects[0].root, root.join("scene-a"));
        assert_eq!(ws.projects[1].root, root.join("scene-b"));
        assert_eq!(ws.member_header(0).unwrap(), "[1/2] in scene-a:");
        assert_eq!(ws.member_header(1).unwrap(), "[2/2] in scene-b:");
    }

    #[test]
    fn malformed_workspace_json_names_the_location() {
        let t = Tmp::new("badjson");
        t.write("dcl-workspace.json", "{\"folders\": [");
        let err = Workspace::load(&t.0).unwrap_err().to_string();
        assert!(err.contains("dcl-workspace.json is not valid JSON"));
        assert!(err.contains("line"));
    }

    #[test]
    fn empty_or_missing_folders_is_a_shape_error() {
        let t = Tmp::new("empty");
        t.write("dcl-workspace.json", r#"{"folders":[]}"#);
        let err = Workspace::load(&t.0).unwrap_err().to_string();
        assert!(err.contains("must list at least one folder"));
        t.write("dcl-workspace.json", r#"{"something":"else"}"#);
        let err = Workspace::load(&t.0).unwrap_err().to_string();
        assert!(err.contains("must list at least one folder"));
        t.write("dcl-workspace.json", r#"{"folders":[{"nopath":true}]}"#);
        let err = Workspace::load(&t.0).unwrap_err().to_string();
        assert!(err.contains("must list at least one folder"));
    }

    #[test]
    fn member_without_scene_json_fails_with_the_scene_error() {
        let t = Tmp::new("badmember");
        t.write("dcl-workspace.json", r#"{"folders":[{"path":"scene-a"}]}"#);
        std::fs::create_dir_all(t.0.join("scene-a")).unwrap();
        let err = format!("{:#}", Workspace::load(&t.0).unwrap_err());
        assert!(err.contains("scene-a"));
        assert!(err.contains("not a Decentraland scene"));
    }

    #[test]
    fn missing_member_folder_names_the_directory() {
        let t = Tmp::new("gone");
        t.write("dcl-workspace.json", r#"{"folders":[{"path":"missing"}]}"#);
        let err = format!("{:#}", Workspace::load(&t.0).unwrap_err());
        assert!(err.contains("does not exist"));
    }
}
