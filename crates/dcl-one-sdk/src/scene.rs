use crate::ux::{TrySteps, UserError};
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Project {
    pub root: PathBuf,
    pub scene_json: Value,
}

impl Project {
    pub fn load(dir: &Path) -> Result<Self> {
        if !dir.is_dir() {
            return Err(UserError::new(
                format!("the directory {} does not exist", dir.display()),
                TrySteps::one("check the path passed to --dir")
                    .and("run the command from inside your scene folder"),
            )
            .into());
        }
        let root = dunce::canonicalize(dir)
            .with_context(|| format!("resolving project dir {}", dir.display()))?;
        let scene_path = root.join("scene.json");
        if !scene_path.is_file() {
            if root.join(crate::workspace::WORKSPACE_FILE).is_file() {
                return Err(UserError::new(
                    "this directory is a workspace root, not a single scene",
                    TrySteps::one(
                        "run this command from inside one of the folders listed in dcl-workspace.json",
                    )
                    .and("build and start understand workspaces \u{2014} run them here to cover every member"),
                )
                .why(format!(
                    "{} exists but scene.json does not",
                    root.join(crate::workspace::WORKSPACE_FILE).display()
                ))
                .into());
            }
            return Err(UserError::new(
                "this directory is not a Decentraland scene",
                TrySteps::one("cd into your scene folder, or pass --dir <path>")
                    .and("start a new scene with: dcl-one-sdk init"),
            )
            .why(format!("no scene.json in {}", root.display()))
            .into());
        }
        let bytes = std::fs::read(&scene_path)
            .with_context(|| format!("reading {}", scene_path.display()))?;
        let scene_json: Value = serde_json::from_slice(&bytes).map_err(|e| {
            UserError::new(
                format!(
                    "scene.json is not valid JSON (line {}, column {})",
                    e.line(),
                    e.column()
                ),
                TrySteps::one(format!(
                    "fix the syntax error at scene.json:{}:{}",
                    e.line(),
                    e.column()
                ))
                .and("validate the file with a JSON linter"),
            )
            .caused_by(e)
        })?;
        let runtime = scene_json
            .get("runtimeVersion")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if runtime != "7" {
            let why = if runtime.is_empty() {
                "scene.json has no runtimeVersion; this tool builds SDK 7 scenes only".to_string()
            } else {
                format!(
                    "scene.json runtimeVersion is \"{runtime}\"; this tool builds SDK 7 scenes only"
                )
            };
            return Err(UserError::new(
                "this scene targets SDK 6, which dcl-one-sdk cannot build",
                TrySteps::one("follow the SDK 7 migration guide in the creator docs")
                    .and("after migrating, set \"runtimeVersion\": \"7\" in scene.json"),
            )
            .why(why)
            .into());
        }
        if let Some(warning) = min_cli_warning(&root) {
            tracing::warn!("{warning}");
        }
        Ok(Self { root, scene_json })
    }

    pub fn main_output(&self) -> Result<String> {
        let main = self
            .scene_json
            .get("main")
            .and_then(|m| m.as_str())
            .unwrap_or_default();
        if main.is_empty() {
            return Err(UserError::new(
                "scene.json is missing \"main\"",
                TrySteps::one("add \"main\": \"bin/index.js\" to scene.json"),
            )
            .why("\"main\" names the bundle file the explorer loads")
            .into());
        }
        if self.root.join(main).is_dir() {
            return Err(UserError::new(
                format!("scene.json \"main\" points at \"{main}\", which is a directory"),
                TrySteps::one("set \"main\": \"bin/index.js\" in scene.json"),
            )
            .why("\"main\" must be the bundle output file")
            .into());
        }
        if !main.ends_with(".js") {
            return Err(UserError::new(
                format!("scene.json \"main\" must be a .js bundle path (got \"{main}\")"),
                TrySteps::one("set \"main\": \"bin/index.js\" in scene.json"),
            )
            .into());
        }
        Ok(main.to_string())
    }

    pub fn parcels(&self) -> Vec<String> {
        self.scene_json
            .get("scene")
            .and_then(|s| s.get("parcels"))
            .and_then(|p| p.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn node_module(&self, rel: &str) -> Option<PathBuf> {
        let p = self.root.join("node_modules").join(rel);
        p.exists().then_some(p)
    }

    pub fn require_node_module(&self, rel: &str) -> Result<PathBuf> {
        match self.node_module(rel) {
            Some(p) => Ok(p),
            None => Err(UserError::new(
                format!("{rel} is not installed in this scene"),
                TrySteps::one("run dcl-one-sdk init --node-modules-only to restore the vendored node_modules (or npm install)"),
            )
            .why(format!(
                "{} does not exist",
                self.root.join("node_modules").join(rel).display()
            ))
            .into()),
        }
    }

    pub fn is_editor_scene(&self) -> bool {
        self.root.join("assets/scene/main.composite").exists()
    }

    pub fn tsconfig(&self) -> Result<PathBuf> {
        let p = self.root.join("tsconfig.json");
        if !p.exists() {
            return Err(UserError::new(
                "this scene has no tsconfig.json",
                TrySteps::one(
                    "create tsconfig.json containing: { \"extends\": \"@dcl/sdk/types/tsconfig.ecs7.json\" }",
                ),
            )
            .why("the bundler and type checker both require it")
            .into());
        }
        Ok(p)
    }
}

pub const TRACKED_MIN_CLI: &str = "3.14.1";

pub fn min_cli_warning(root: &Path) -> Option<String> {
    let declared = package_min_cli(&root.join("package.json"))
        .or_else(|| package_min_cli(&root.join("node_modules/@dcl/sdk/package.json")))?;
    let min = parse_semver(&declared)?;
    let tracked = parse_semver(TRACKED_MIN_CLI)?;
    if min > tracked {
        Some(format!(
            "this project asks for CLI version >= {declared}, newer than the {TRACKED_MIN_CLI} level dcl-one-sdk tracks (@dcl/sdk-commands 7.22.6) \u{2014} if a command misbehaves, cross-check with npx @dcl/sdk-commands"
        ))
    } else {
        None
    }
}

fn package_min_cli(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    if let Some(s) = v.get("minCliVersion").and_then(|s| s.as_str()) {
        return Some(s.to_string());
    }
    v.get("engines")
        .and_then(|e| e.get("minCliVersion"))
        .and_then(|s| s.as_str())
        .map(str::to_string)
}

fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
    let core = s
        .trim()
        .trim_start_matches(['>', '=', '~', '^', 'v', ' '])
        .split(['-', '+'])
        .next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

pub fn machine_id() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "dcl-one".to_string())
}

pub fn b64_hash(path_str: &str, machine: &str) -> String {
    use base64::Engine;
    let unique = format!("{path_str}-{machine}");
    format!(
        "b64-{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(unique.as_bytes())
    )
}

pub fn b64_unhash(hash: &str, machine: &str) -> Option<String> {
    use base64::Engine;
    let b = hash.strip_prefix("b64-")?;
    let normalized = b.trim_end_matches('=').replace('+', "-").replace('/', "_");
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(normalized.as_bytes())
        .ok()?;
    let s = String::from_utf8(decoded).ok()?;
    s.strip_suffix(&format!("-{machine}")).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);

    impl Tmp {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("dcl-one-sdk-mincli-{tag}-{}", std::process::id()));
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
    fn no_package_json_is_silent() {
        let t = Tmp::new("none");
        assert_eq!(min_cli_warning(&t.0), None);
    }

    #[test]
    fn tracked_level_is_silent() {
        let t = Tmp::new("ok");
        t.write("package.json", r#"{"minCliVersion":"3.14.1"}"#);
        assert_eq!(min_cli_warning(&t.0), None);
    }

    #[test]
    fn newer_min_warns_and_names_both_versions() {
        let t = Tmp::new("newer");
        t.write("package.json", r#"{"minCliVersion":"3.15.0"}"#);
        let w = min_cli_warning(&t.0).unwrap();
        assert!(w.contains("3.15.0"));
        assert!(w.contains(TRACKED_MIN_CLI));
        assert!(w.contains("@dcl/sdk-commands"));
    }

    #[test]
    fn engines_form_and_range_prefixes_parse() {
        let t = Tmp::new("engines");
        t.write("package.json", r#"{"engines":{"minCliVersion":">=4.0.0"}}"#);
        assert!(min_cli_warning(&t.0).is_some());
    }

    #[test]
    fn installed_sdk_manifest_is_the_fallback_source() {
        let t = Tmp::new("sdkfall");
        t.write(
            "node_modules/@dcl/sdk/package.json",
            r#"{"minCliVersion":"3.99.0"}"#,
        );
        assert!(min_cli_warning(&t.0).is_some());
        t.write("package.json", r#"{"minCliVersion":"3.0.0"}"#);
        assert_eq!(min_cli_warning(&t.0), None);
    }

    #[test]
    fn unparseable_versions_stay_silent() {
        let t = Tmp::new("garbage");
        t.write("package.json", r#"{"minCliVersion":"latest"}"#);
        assert_eq!(min_cli_warning(&t.0), None);
    }

    #[test]
    fn semver_compare_is_numeric_not_lexical() {
        assert!(parse_semver("3.9.0").unwrap() < parse_semver("3.14.1").unwrap());
        assert!(parse_semver("10.0.0").unwrap() > parse_semver("9.9.9").unwrap());
        assert_eq!(
            parse_semver("7.22.6-25007982108.commit-83012ab").unwrap(),
            (7, 22, 6)
        );
    }
}
