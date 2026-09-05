use crate::ux::{self, write_error, TrySteps, UserError};
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

const INSTALLED_NOTE: &str = "Installed node_modules from the vendored SDK — no npm needed";

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
        steps.done(match install_vendored_node_modules(&root)? {
            true => INSTALLED_NOTE,
            false => "node_modules already exists — nothing to do",
        });
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
    steps.done(match install_vendored_node_modules(&root)? {
        true => INSTALLED_NOTE,
        false => "Kept the existing node_modules",
    });
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

/// Is a full `@dcl/inspector` (with the editor UI) present? The base blob ships
/// a stand-in that only implements the crdt dump, so the UI bundle decides.
pub fn has_full_inspector(root: &Path) -> bool {
    root.join("node_modules/@dcl/inspector/public/index.html")
        .is_file()
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
    };
    let file = |rel, body: String| FileSpec {
        rel,
        body: body.into_bytes(),
    };
    let raw = |rel, body: &str| file(rel, body.to_string());
    let (head, index, readme): (Vec<FileSpec>, _, _) = match kind {
        ProjectKind::Scene => (
            vec![
                file("scene.json", sub(SCENE_SCENE_JSON)),
                file("package.json", sub(SCENE_PACKAGE_JSON)),
            ],
            SCENE_INDEX_TS,
            SCENE_README,
        ),
        ProjectKind::SmartWearable => (
            vec![
                file(
                    "wearable.json",
                    sub(SW_WEARABLE_JSON).replace("{{ID}}", &uuid_v4()),
                ),
                file(
                    "scene.json",
                    sub(SW_SCENE_JSON).replace("{{PARCELS}}", &parcel_grid(10, 10)),
                ),
                file("package.json", sub(SW_PACKAGE_JSON)),
            ],
            SW_INDEX_TS,
            SW_README,
        ),
    };
    let mut files = head;
    files.extend([
        raw("tsconfig.json", SCENE_TSCONFIG),
        file("src/index.ts", sub(index)),
        raw(".gitignore", SCENE_GITIGNORE),
        raw(".dclignore", SCENE_DCLIGNORE),
        file("README.md", sub(readme)),
    ]);
    if kind == ProjectKind::Scene {
        files.push(FileSpec {
            rel: "images/scene-thumbnail.png",
            body: SCENE_THUMBNAIL.to_vec(),
        });
    }
    files
}

fn write_file(root: &Path, rel: &str, body: &[u8]) -> Result<()> {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| write_error(&path, e))?;
    }
    std::fs::write(&path, body).map_err(|e| write_error(&path, e))?;
    Ok(())
}

fn display_dir(dir: &Path) -> String {
    match dir == Path::new(".") {
        true => "the current directory".to_string(),
        false => dir.display().to_string(),
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
    match cleaned.trim().trim_matches('-').trim() {
        "" => "my-scene".to_string(),
        t => t.to_string(),
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
    match slug.is_empty() {
        true => "new-scene".to_string(),
        false => slug,
    }
}

fn parcel_grid(cols: u32, rows: u32) -> String {
    (0..rows)
        .flat_map(|y| (0..cols).map(move |x| format!("\"{x},{y}\"")))
        .collect::<Vec<_>>()
        .join(", ")
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

    /// Each of these is a silent failure if a blob rebuild drops it: without
    /// `host.js` `start --data-layer` cannot boot, a descriptor short of 22
    /// methods is a `TypeError` that kills the whole rpc connection, and
    /// without `component-schemas.json` the engine drops every crdt message
    /// for a component it does not know.
    #[test]
    fn blob_carries_the_data_layer_host() {
        let cursor = std::io::Cursor::new(VENDORED_NODE_MODULES);
        let mut archive = zip::ZipArchive::new(cursor).unwrap();
        let names: Vec<String> = archive.file_names().map(str::to_string).collect();
        for rel in [
            "node_modules/@dcl/inspector/host.js",
            "node_modules/@dcl/inspector/engine.js",
            "node_modules/@dcl/inspector/engine-to-composite.js",
            "node_modules/@dcl/inspector/data-layer.gen.js",
            "node_modules/@dcl/inspector/component-schemas.json",
            "node_modules/@dcl/inspector/minimal-composite.json",
            "node_modules/@dcl/rpc/dist/index.js",
            "node_modules/@dcl/rpc/dist/codegen.js",
            "node_modules/@dcl/rpc/dist/push-channel.js",
            "node_modules/@dcl/rpc/dist/transports/WebSocket.js",
            "node_modules/mitt/dist/mitt.js",
        ] {
            assert!(names.iter().any(|n| n == rel), "blob is missing {rel}");
        }
        assert!(
            !names
                .iter()
                .any(|n| n.ends_with(".gen.ts") || n.ends_with(".proto")),
            "the descriptor source leaked into the blob"
        );

        use std::io::Read;
        let mut descriptor = String::new();
        archive
            .by_name("node_modules/@dcl/inspector/data-layer.gen.js")
            .unwrap()
            .read_to_string(&mut descriptor)
            .unwrap();
        assert_eq!(
            descriptor.matches("requestStream:").count(),
            22,
            "the DataService descriptor must declare all 22 methods"
        );
    }

    /// Both scaffold pins must name the blob's @dcl line (a manifest naming an
    /// older one would have the next npm install downgrade it), and the blob's
    /// @dcl/ecs must carry the 7.27.0 fixes: the byteLength-scoped DataView
    /// (upstream #1460), the renderer-reserved entity-id guard (#1544), and —
    /// ours until upstream #1595 ships — the eight-byte network delete body the
    /// bevy engine's strict framing needs (`patch_ecs_network_delete_length()`
    /// in scripts/blob_overlays.py).
    #[test]
    fn blob_tracks_the_scaffold_pin_and_carries_the_ecs_fixes() {
        use std::io::Read;
        let scene: serde_json::Value = serde_json::from_str(SCENE_PACKAGE_JSON).unwrap();
        let pin = scene["devDependencies"]["@dcl/sdk"]
            .as_str()
            .unwrap()
            .to_string();
        for (name, raw) in [
            ("scene", SCENE_PACKAGE_JSON),
            ("smart-wearable", SW_PACKAGE_JSON),
        ] {
            let scaffold: serde_json::Value = serde_json::from_str(raw).unwrap();
            for dep in ["@dcl/sdk", "@dcl/js-runtime"] {
                assert_eq!(
                    scaffold["devDependencies"][dep], pin,
                    "the {name} scaffold's {dep} is off the blob's line"
                );
            }
        }

        let cursor = std::io::Cursor::new(VENDORED_NODE_MODULES);
        let mut archive = zip::ZipArchive::new(cursor).unwrap();
        let mut read = |rel: &str| {
            let mut s = String::new();
            archive
                .by_name(rel)
                .unwrap_or_else(|e| panic!("blob is missing {rel}: {e}"))
                .read_to_string(&mut s)
                .unwrap();
            s
        };
        for pkg in ["@dcl/sdk", "@dcl/ecs", "@dcl/js-runtime"] {
            let manifest: serde_json::Value =
                serde_json::from_str(&read(&format!("node_modules/{pkg}/package.json"))).unwrap();
            assert_eq!(
                manifest["version"], pin,
                "{pkg} is not on the scaffold's line"
            );
        }

        let byte_buffer = read("node_modules/@dcl/ecs/dist-cjs/serialization/ByteBuffer/index.js");
        assert_eq!(
            byte_buffer
                .matches("new DataView(this._buffer.buffer, this._buffer.byteOffset, this._buffer.byteLength)")
                .count(),
            2,
            "ByteBuffer must scope both DataViews to the buffer's byteLength"
        );
        assert!(
            !byte_buffer.contains("new DataView(this._buffer.buffer, oldOffset)"),
            "ByteBuffer still rebuilds its view from the pre-growth offset"
        );

        let entity = read("node_modules/@dcl/ecs/dist-cjs/engine/entity.js");
        assert!(
            entity.contains("function isReservedEntity("),
            "entity container lacks the renderer-reserved id guard"
        );
        assert!(
            entity.contains("number >= reservedStaticEntities && version < exports.MAX_U16"),
            "entity container still recycles renderer-reserved numbers"
        );

        let core = read("node_modules/@dcl/sdk/prebuilt/core.js");
        assert_eq!(
            core.matches(
                "new DataView(this._buffer.buffer,this._buffer.byteOffset,this._buffer.byteLength)"
            )
            .count(),
            2,
            "the prebuilt runtime chunk was not built from the fixed ByteBuffer"
        );

        let net_delete = read(
            "node_modules/@dcl/ecs/dist-cjs/serialization/crdt/network/deleteEntityNetwork.js",
        );
        assert!(
            net_delete.contains(
                "buf.writeUint32(types_1.CRDT_MESSAGE_HEADER_LENGTH + DeleteEntityNetwork.MESSAGE_HEADER_LENGTH);"
            ),
            "DeleteEntityNetwork.write does not declare the eight-byte body it writes (upstream #1595)"
        );
        assert!(
            !net_delete.contains("CRDT_MESSAGE_HEADER_LENGTH + 4"),
            "DeleteEntityNetwork.write still declares a 12-byte record"
        );
        let write = chunk_net_delete_write(&core);
        assert!(
            write.contains("writeUint32(8+") && write.contains(".MESSAGE_HEADER_LENGTH),"),
            "the prebuilt runtime chunk frames a network entity delete short: {write}"
        );
        assert!(
            !write.contains("writeUint32(12),"),
            "the prebuilt runtime chunk was built from the unpatched @dcl/ecs: {write}"
        );
    }

    /// The minified `DeleteEntityNetwork.write`. Rolldown mangles every local
    /// name, so the anchors are the two things it keeps: the namespace's
    /// `MESSAGE_HEADER_LENGTH=8` (its siblings set 4, 12, 16 or 20) and the
    /// string literal `read` throws; the write function sits between them.
    fn chunk_net_delete_write(core: &str) -> &str {
        let end = core
            .find("DeleteEntityNetwork tried to read another message type")
            .expect("core.js lacks the DeleteEntityNetwork namespace");
        let start = core[..end]
            .rfind("MESSAGE_HEADER_LENGTH=8;")
            .expect("core.js lacks DeleteEntityNetwork.MESSAGE_HEADER_LENGTH");
        &core[start..end]
    }
}
