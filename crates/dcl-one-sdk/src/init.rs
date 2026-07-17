use crate::ux::{self, TrySteps, UserError};
use anyhow::{Context, Result};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum ProjectKind {
    Scene,
    SmartWearable,
}

impl ProjectKind {
    fn label(self) -> &'static str {
        match self {
            ProjectKind::Scene => "scene",
            ProjectKind::SmartWearable => "smart wearable",
        }
    }
}

pub struct InitOptions {
    pub dir: PathBuf,
    pub project: Option<ProjectKind>,
    pub yes: bool,
    pub node_modules_only: bool,
}

pub struct FileSpec {
    pub rel: &'static str,
    pub body: Vec<u8>,
}

const SCENE_SCENE_JSON: &str = include_str!("templates/init/scene/scene.json");
const SCENE_PACKAGE_JSON: &str = include_str!("templates/init/scene/package.json");
const SCENE_TSCONFIG: &str = include_str!("templates/init/scene/tsconfig.json");
const SCENE_INDEX_TS: &str = include_str!("templates/init/scene/index.ts");
const SCENE_GITIGNORE: &str = include_str!("templates/init/scene/gitignore");
const SCENE_DCLIGNORE: &str = include_str!("templates/init/scene/dclignore");
const SCENE_README: &str = include_str!("templates/init/scene/README.md");
const SCENE_THUMBNAIL: &[u8] = include_bytes!("templates/init/scene/scene-thumbnail.png");
const SW_WEARABLE_JSON: &str = include_str!("templates/init/smart-wearable/wearable.json");
const SW_SCENE_JSON: &str = include_str!("templates/init/smart-wearable/scene.json");
const SW_PACKAGE_JSON: &str = include_str!("templates/init/smart-wearable/package.json");
const SW_INDEX_TS: &str = include_str!("templates/init/smart-wearable/index.ts");
const SW_README: &str = include_str!("templates/init/smart-wearable/README.md");
const VENDORED_NODE_MODULES: &[u8] = include_bytes!("vendor/node_modules.zip");

pub fn init(opts: &InitOptions) -> Result<()> {
    if opts.node_modules_only {
        let root = dunce::canonicalize(&opts.dir).map_err(|e| {
            UserError::new(
                format!("cannot resolve the target directory {}", opts.dir.display()),
                TrySteps::one("run from inside the scene, or pass --dir <scene>"),
            )
            .caused_by(e)
        })?;
        let mut steps = ux::Steps::new(1);
        if install_vendored_node_modules(&root)? {
            steps.done("Installed node_modules from the vendored SDK — no npm needed");
        } else {
            steps.done("node_modules already exists — nothing to do");
        }
        return Ok(());
    }
    let root = prepare_dir(&opts.dir, opts.yes)?;
    let kind = resolve_kind(opts.project)?;
    let title = project_title(&root);
    let files = scaffold_files(kind, &title);
    for f in &files {
        write_file(&root, f.rel, &f.body)?;
    }
    let mut steps = ux::Steps::new(3);
    steps.done(format!(
        "Scaffolded a {} project in {} ({} files)",
        kind.label(),
        display_dir(&opts.dir),
        files.len()
    ));
    if install_vendored_node_modules(&root)? {
        steps.done("Installed node_modules from the vendored SDK — no npm needed");
    } else {
        steps.done("Kept the existing node_modules");
    }
    steps.done("Next steps:");
    if opts.dir != Path::new(".") {
        ux::note(format!("  cd {}", display_dir(&opts.dir)));
    }
    ux::note("  dcl-one-sdk start");
    match kind {
        ProjectKind::Scene => {
            ux::note("  dcl-one-sdk deploy   when you are ready to publish");
        }
        ProjectKind::SmartWearable => {
            ux::note("  add model.glb and thumbnail.png (256x256, transparent background) — wearable.json references them");
            ux::note("  dcl-one-sdk pack     to produce smart-wearable.zip when you are ready to publish");
        }
    }
    Ok(())
}

fn install_vendored_node_modules(root: &Path) -> Result<bool> {
    if root.join("node_modules").exists() {
        return Ok(false);
    }
    let cursor = std::io::Cursor::new(VENDORED_NODE_MODULES);
    let mut archive = zip::ZipArchive::new(cursor).context("opening the vendored node_modules")?;
    archive
        .extract(root)
        .context("extracting the vendored node_modules")?;
    Ok(true)
}

fn prepare_dir(dir: &Path, yes: bool) -> Result<PathBuf> {
    if dir.is_file() {
        return Err(UserError::new(
            format!(
                "the target path {} is a file, not a directory",
                dir.display()
            ),
            TrySteps::one("pass a directory to --dir, or run init from inside an empty folder"),
        )
        .into());
    }
    std::fs::create_dir_all(dir).map_err(|e| {
        UserError::new(
            format!("cannot create the target directory {}", dir.display()),
            TrySteps::one("check write permission on the parent directory"),
        )
        .caused_by(e)
    })?;
    let root = dunce::canonicalize(dir)
        .with_context(|| format!("resolving target dir {}", dir.display()))?;
    let mut entries: Vec<String> = std::fs::read_dir(&root)
        .map_err(|e| {
            UserError::new(
                format!("cannot read the target directory {}", root.display()),
                TrySteps::one("check read permission on the directory"),
            )
            .caused_by(e)
        })?
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect();
    if !entries.is_empty() && !yes {
        entries.sort();
        return Err(UserError::new(
            "the target directory is not empty",
            TrySteps::one(
                "run init in a fresh folder: mkdir my-scene && dcl-one-sdk init --dir my-scene",
            )
            .and(
                "or pass --yes to scaffold here anyway (files with template names get overwritten)",
            ),
        )
        .why(format!(
            "{} contains {} entries (first: {})",
            root.display(),
            entries.len(),
            entries[0]
        ))
        .into());
    }
    Ok(root)
}

fn resolve_kind(flag: Option<ProjectKind>) -> Result<ProjectKind> {
    if let Some(kind) = flag {
        return Ok(kind);
    }
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        return prompt_kind();
    }
    ux::note(
        "no --project given and no terminal to ask — scaffolding the default scene project (pass --project scene|smart-wearable to choose)",
    );
    Ok(ProjectKind::Scene)
}

fn prompt_kind() -> Result<ProjectKind> {
    println!("What would you like to create?");
    println!("  1) scene           a standard Decentraland scene (default)");
    println!("  2) smart-wearable  a wearable with its own portable-experience code");
    print!("Choose [1/2] (enter = 1): ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .context("reading the project kind answer")?;
    parse_kind_choice(line.trim())
}

fn parse_kind_choice(answer: &str) -> Result<ProjectKind> {
    match answer.to_ascii_lowercase().as_str() {
        "" | "1" | "scene" => Ok(ProjectKind::Scene),
        "2" | "smart-wearable" | "smart wearable" | "wearable" => Ok(ProjectKind::SmartWearable),
        other => Err(UserError::new(
            format!("\"{other}\" is not a project kind"),
            TrySteps::one("answer 1 (scene) or 2 (smart-wearable)")
                .and("or skip the prompt: dcl-one-sdk init --project scene"),
        )
        .into()),
    }
}

pub fn scaffold_files(kind: ProjectKind, title: &str) -> Vec<FileSpec> {
    let slug = project_slug(title);
    let description = match kind {
        ProjectKind::Scene => "A new Decentraland scene.",
        ProjectKind::SmartWearable => "A new Decentraland smart wearable.",
    };
    let sub = |template: &str| {
        template
            .replace("{{TITLE}}", title)
            .replace("{{DESCRIPTION}}", description)
            .replace("{{SLUG}}", &slug)
            .into_bytes()
    };
    match kind {
        ProjectKind::Scene => vec![
            FileSpec {
                rel: "scene.json",
                body: sub(SCENE_SCENE_JSON),
            },
            FileSpec {
                rel: "package.json",
                body: sub(SCENE_PACKAGE_JSON),
            },
            FileSpec {
                rel: "tsconfig.json",
                body: SCENE_TSCONFIG.as_bytes().to_vec(),
            },
            FileSpec {
                rel: "src/index.ts",
                body: sub(SCENE_INDEX_TS),
            },
            FileSpec {
                rel: ".gitignore",
                body: SCENE_GITIGNORE.as_bytes().to_vec(),
            },
            FileSpec {
                rel: ".dclignore",
                body: SCENE_DCLIGNORE.as_bytes().to_vec(),
            },
            FileSpec {
                rel: "README.md",
                body: sub(SCENE_README),
            },
            FileSpec {
                rel: "images/scene-thumbnail.png",
                body: SCENE_THUMBNAIL.to_vec(),
            },
        ],
        ProjectKind::SmartWearable => vec![
            FileSpec {
                rel: "wearable.json",
                body: String::from_utf8(sub(SW_WEARABLE_JSON))
                    .expect("wearable template is utf8")
                    .replace("{{ID}}", &uuid_v4())
                    .into_bytes(),
            },
            FileSpec {
                rel: "scene.json",
                body: String::from_utf8(sub(SW_SCENE_JSON))
                    .expect("scene template is utf8")
                    .replace("{{PARCELS}}", &parcel_grid(10, 10))
                    .into_bytes(),
            },
            FileSpec {
                rel: "package.json",
                body: sub(SW_PACKAGE_JSON),
            },
            FileSpec {
                rel: "tsconfig.json",
                body: SCENE_TSCONFIG.as_bytes().to_vec(),
            },
            FileSpec {
                rel: "src/index.ts",
                body: sub(SW_INDEX_TS),
            },
            FileSpec {
                rel: ".gitignore",
                body: SCENE_GITIGNORE.as_bytes().to_vec(),
            },
            FileSpec {
                rel: ".dclignore",
                body: SCENE_DCLIGNORE.as_bytes().to_vec(),
            },
            FileSpec {
                rel: "README.md",
                body: sub(SW_README),
            },
        ],
    }
}

fn write_file(root: &Path, rel: &str, body: &[u8]) -> Result<()> {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| write_error(&path, e))?;
    }
    std::fs::write(&path, body).map_err(|e| write_error(&path, e))?;
    Ok(())
}

fn write_error(path: &Path, e: std::io::Error) -> anyhow::Error {
    UserError::new(
        format!("cannot write to {}", path.display()),
        TrySteps::one("check write permission on the project directory")
            .and("re-run from a writable checkout (not a read-only mount)"),
    )
    .caused_by(e)
    .into()
}

fn display_dir(dir: &Path) -> String {
    if dir == Path::new(".") {
        "the current directory".to_string()
    } else {
        dir.display().to_string()
    }
}

pub fn project_title(root: &Path) -> String {
    let raw = root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || " ._-".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('-').trim().to_string();
    if trimmed.is_empty() {
        "my-scene".to_string()
    } else {
        trimmed
    }
}

pub fn project_slug(name: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for c in name.trim().to_ascii_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(c);
        } else {
            pending_dash = true;
        }
    }
    if slug.is_empty() {
        "new-scene".to_string()
    } else {
        slug
    }
}

fn parcel_grid(cols: u32, rows: u32) -> String {
    let mut out = Vec::new();
    for y in 0..rows {
        for x in 0..cols {
            out.push(format!("\"{x},{y}\""));
        }
    }
    out.join(", ")
}

fn uuid_v4() -> String {
    let mut b: [u8; 16] = rand::random();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_matches_the_ported_projectslug() {
        assert_eq!(project_slug("My Awesome Scene"), "my-awesome-scene");
        assert_eq!(project_slug("  spaced  out  "), "spaced-out");
        assert_eq!(project_slug("??"), "new-scene");
        assert_eq!(project_slug(""), "new-scene");
        assert_eq!(project_slug("-Already-Slugged-"), "already-slugged");
    }

    #[test]
    fn title_sanitizes_shell_hostile_names() {
        assert_eq!(project_title(Path::new("/tmp/my-scene")), "my-scene");
        assert_eq!(project_title(Path::new("/tmp/a\"b\\c")), "a-b-c");
        assert_eq!(project_title(Path::new("/")), "my-scene");
    }

    #[test]
    fn kind_choice_accepts_numbers_names_and_default() {
        assert_eq!(parse_kind_choice("").unwrap(), ProjectKind::Scene);
        assert_eq!(parse_kind_choice("1").unwrap(), ProjectKind::Scene);
        assert_eq!(parse_kind_choice("Scene").unwrap(), ProjectKind::Scene);
        assert_eq!(parse_kind_choice("2").unwrap(), ProjectKind::SmartWearable);
        assert_eq!(
            parse_kind_choice("wearable").unwrap(),
            ProjectKind::SmartWearable
        );
        assert!(parse_kind_choice("library").is_err());
    }

    #[test]
    fn parcel_grid_is_the_full_10x10() {
        let grid = parcel_grid(10, 10);
        assert!(grid.starts_with("\"0,0\", \"1,0\""));
        assert!(grid.ends_with("\"9,9\""));
        assert_eq!(grid.matches(',').count(), 199);
    }

    #[test]
    fn uuid_v4_shape() {
        let id = uuid_v4();
        assert_eq!(id.len(), 36);
        assert_eq!(id.as_bytes()[14], b'4');
        for i in [8, 13, 18, 23] {
            assert_eq!(id.as_bytes()[i], b'-');
        }
    }

    #[test]
    fn scene_scaffold_substitutes_every_placeholder() {
        for f in scaffold_files(ProjectKind::Scene, "Test Scene") {
            let body = String::from_utf8_lossy(&f.body).into_owned();
            assert!(!body.contains("{{"), "{} still has a placeholder", f.rel);
        }
    }

    #[test]
    fn wearable_scaffold_substitutes_every_placeholder() {
        for f in scaffold_files(ProjectKind::SmartWearable, "Test Wearable") {
            let body = String::from_utf8_lossy(&f.body).into_owned();
            assert!(!body.contains("{{"), "{} still has a placeholder", f.rel);
        }
    }
}
