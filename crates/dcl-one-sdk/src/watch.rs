use crate::build::BuildOptions;
use crate::entrypoint;
use crate::esbuild::{self, EsbuildOptions};
use crate::live_reload::ReloadEvent;
use crate::scene::Project;
use crate::split;
use crate::ux::{self, TrySteps, UserError};
use anyhow::Result;
use notify::{RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const DEBOUNCE: Duration = Duration::from_millis(100);

pub struct FsWatcher {
    _watcher: notify::RecommendedWatcher,
    rx: mpsc::UnboundedReceiver<PathBuf>,
    root: PathBuf,
}

impl FsWatcher {
    pub fn new(root: &Path) -> Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel::<PathBuf>();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                if matches!(event.kind, notify::EventKind::Access(_)) {
                    return;
                }
                for path in event.paths {
                    let _ = tx.send(path);
                }
            }
        })
        .map_err(|e| {
            anyhow::Error::from(
                UserError::new(
                    "could not start the file watcher",
                    TrySteps::one(
                        "on Linux, raise the inotify limit: sudo sysctl fs.inotify.max_user_instances=512",
                    )
                    .and("to build once without watching, run dcl-one-sdk build"),
                )
                .caused_by(e),
            )
        })?;
        watcher.watch(root, RecursiveMode::Recursive).map_err(|e| {
            anyhow::Error::from(
                UserError::new(
                    format!(
                        "could not watch {} for changes (system watch limit reached?)",
                        root.display()
                    ),
                    TrySteps::one(
                        "raise the limit: sudo sysctl fs.inotify.max_user_watches=524288",
                    )
                    .and("or run dcl-one-sdk start --no-watch"),
                )
                .caused_by(e),
            )
        })?;
        Ok(Self {
            _watcher: watcher,
            rx,
            root: root.to_path_buf(),
        })
    }

    pub async fn next_batch(&mut self) -> Option<Vec<PathBuf>> {
        loop {
            let first = self.rx.recv().await?;
            let mut batch = Vec::new();
            if is_relevant(&self.root, &first) {
                batch.push(first);
            }
            let deadline = tokio::time::Instant::now() + DEBOUNCE;
            loop {
                match tokio::time::timeout_at(deadline, self.rx.recv()).await {
                    Ok(Some(p)) => {
                        if is_relevant(&self.root, &p) {
                            batch.push(p);
                        }
                    }
                    Ok(None) => return (!batch.is_empty()).then_some(batch),
                    Err(_) => break,
                }
            }
            if !batch.is_empty() {
                return Some(batch);
            }
        }
    }
}

pub fn is_relevant(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    let first = rel.components().next().and_then(|c| c.as_os_str().to_str());
    if matches!(first, Some(".dcl-one" | "node_modules" | "bin" | ".git")) {
        return false;
    }
    if is_model(path) {
        return true;
    }
    matches!(
        path.extension().and_then(|e| e.to_str()).unwrap_or(""),
        "ts" | "tsx" | "js" | "jsx" | "composite"
    )
}

pub fn is_model(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("glb") || e.eq_ignore_ascii_case("gltf"))
}

fn partition_batch(paths: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut models, code): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| is_model(p));
    models.sort();
    models.dedup();
    (models, code)
}

struct SplitState {
    sdk_opts: EsbuildOptions,
    registry: Vec<&'static str>,
    generated_dir: PathBuf,
}

pub struct WatchSession {
    project: Project,
    es_opts: EsbuildOptions,
    ignore_composite: bool,
    custom_entry_point: bool,
    split: SplitState,
    outfile: PathBuf,
    sdk_rel: String,
    scene_rel: String,
    max_composite_entity: u32,
}

impl WatchSession {
    pub async fn create(
        project: Project,
        opts: &BuildOptions,
        initial_build: bool,
        steps: &mut ux::Steps,
    ) -> Result<Self> {
        let main = project.main_output()?;
        let outfile = project.root.join(&main);
        let (sdk_rel, scene_rel) = split::chunk_rel_paths(&main);
        let generated = entrypoint::generate(
            &project,
            opts.ignore_composite,
            opts.custom_entry_point,
            true,
        )?;
        split::write_generated(&project, &generated.dir)?;
        split::write_marker(&generated.dir)?;
        split::write_loader_stub(
            &outfile,
            &sdk_rel,
            &scene_rel,
            generated.max_composite_entity,
        )?;
        tracing::info!("loader stub saved {}", outfile.display());
        if initial_build {
            steps.done(format!(
                "Loader stub saved {}",
                ux::rel_to(&project.root, &outfile)
            ));
        }
        let mut sdk_aliases = esbuild::resolve_aliases(&project)?;
        sdk_aliases.push((
            "~sdk/all-composites".to_string(),
            generated.dir.join("composite-slot.js"),
        ));
        sdk_aliases.push((
            "~sdk/script-utils".to_string(),
            generated.dir.join("script-utils.js"),
        ));
        let sdk_opts = EsbuildOptions {
            production: opts.production,
            entrypoint: generated.dir.join("sdk-runtime-entry.js"),
            outfile: project.root.join(&sdk_rel),
            tsconfig: project.tsconfig()?,
            aliases: sdk_aliases,
            externals: vec![],
        };
        let scene_opts = EsbuildOptions {
            production: opts.production,
            entrypoint: generated.entrypoint,
            outfile: project.root.join(&scene_rel),
            tsconfig: project.tsconfig()?,
            aliases: vec![],
            externals: split::scene_externals(&project),
        };
        if initial_build {
            let started = Instant::now();
            esbuild::bundle(&project, &sdk_opts).await?;
            tracing::info!("sdk chunk saved {}", sdk_opts.outfile.display());
            steps.done(format!(
                "SDK chunk saved {} ({})",
                ux::rel_to(&project.root, &sdk_opts.outfile),
                ux::fmt_elapsed(started.elapsed())
            ));
            let started = Instant::now();
            esbuild::bundle(&project, &scene_opts).await?;
            tracing::info!("scene chunk saved {}", scene_opts.outfile.display());
            steps.done(format!(
                "Scene chunk saved {} ({})",
                ux::rel_to(&project.root, &scene_opts.outfile),
                ux::fmt_elapsed(started.elapsed())
            ));
        }
        let registry = split::registry_keys(&project);
        Ok(Self {
            project,
            es_opts: scene_opts,
            ignore_composite: opts.ignore_composite,
            custom_entry_point: opts.custom_entry_point,
            split: SplitState {
                sdk_opts,
                registry,
                generated_dir: generated.dir,
            },
            outfile,
            sdk_rel,
            scene_rel,
            max_composite_entity: generated.max_composite_entity,
        })
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub async fn run(mut self, mut fs: FsWatcher, notify: impl Fn(ReloadEvent)) -> Result<()> {
        loop {
            let Some(batch) = fs.next_batch().await else {
                break;
            };
            let (models, paths) = partition_batch(batch);
            note_models(&self.project.root, &models);
            for model in models {
                notify(ReloadEvent::Model(model));
            }
            if paths.is_empty() {
                continue;
            }
            let started = Instant::now();
            match regenerate_composites(
                &self.project,
                self.ignore_composite,
                self.custom_entry_point,
                &paths,
            )
            .await
            {
                Err(e) => {
                    ux::report_watch(&watch_regen_error(
                        e,
                        "composite rebuild failed \u{2014} watching continues",
                    ));
                    continue;
                }
                Ok(Some(new_max)) if new_max != self.max_composite_entity => {
                    self.max_composite_entity = new_max;
                    if let Err(e) = split::write_loader_stub(
                        &self.outfile,
                        &self.sdk_rel,
                        &self.scene_rel,
                        new_max,
                    ) {
                        ux::report_watch(&e);
                    }
                }
                Ok(_) => {}
            }
            refresh_sdk_chunk_cli(&self.project, &mut self.split).await;
            match esbuild::bundle(&self.project, &self.es_opts).await {
                Ok(()) => {
                    tracing::info!(
                        "rebuilt {} in {:.0?}",
                        self.es_opts.outfile.display(),
                        started.elapsed()
                    );
                    ux::note(format!(
                        "\u{21bb} rebuilt {} ({})",
                        ux::rel_to(&self.project.root, &self.es_opts.outfile),
                        ux::fmt_elapsed(started.elapsed())
                    ));
                    notify(ReloadEvent::Scene);
                }
                Err(e) => ux::report_watch(&e),
            }
        }
        Ok(())
    }
}

fn watch_regen_error(e: anyhow::Error, what: &str) -> anyhow::Error {
    UserError::new(
        what.to_string(),
        TrySteps::one("fix the file named below, then save any file to retry"),
    )
    .why(format!("{e:#}"))
    .into()
}

fn note_models(root: &Path, models: &[PathBuf]) {
    for model in models {
        ux::note(format!("\u{21bb} model update {}", ux::rel_to(root, model)));
    }
}

async fn regenerate_composites(
    project: &Project,
    ignore_composite: bool,
    custom_entry_point: bool,
    paths: &[PathBuf],
) -> Result<Option<u32>> {
    let touched = paths
        .iter()
        .any(|p| p.extension().and_then(|e| e.to_str()) == Some("composite"));
    if !touched {
        return Ok(None);
    }
    let generated = entrypoint::generate(project, ignore_composite, custom_entry_point, true)?;
    tracing::info!("composites changed, regenerated all-composites.js");
    if let Err(e) = crate::data_layer::regenerate_main_crdt(&project.root, ignore_composite).await {
        ux::report_watch(&e);
    }
    Ok(Some(generated.max_composite_entity))
}

async fn refresh_sdk_chunk_cli(project: &Project, sp: &mut SplitState) {
    let keys = split::registry_keys(project);
    if keys == sp.registry {
        return;
    }
    if let Err(e) = split::write_generated(project, &sp.generated_dir) {
        ux::report_watch(&watch_regen_error(
            e,
            "sdk runtime entry rebuild failed \u{2014} watching continues",
        ));
        return;
    }
    match esbuild::bundle(project, &sp.sdk_opts).await {
        Ok(()) => {
            sp.registry = keys;
            tracing::info!(
                "sdk registry changed, rebuilt {}",
                sp.sdk_opts.outfile.display()
            );
            ux::note(format!(
                "\u{21bb} rebuilt {} (sdk registry changed)",
                ux::rel_to(&project.root, &sp.sdk_opts.outfile)
            ));
        }
        Err(e) => ux::report_watch(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::is_relevant;
    use std::path::Path;

    fn under_root(rel: &str) -> bool {
        let root = Path::new("/proj");
        is_relevant(root, &root.join(rel))
    }

    #[test]
    fn build_output_and_tool_dirs_are_ignored_by_component() {
        assert!(!under_root("bin/index.js"));
        assert!(!under_root("bin/scene.js"));
        assert!(!under_root("node_modules/foo/bar.js"));
        assert!(!under_root(".dcl-one/all-composites.js"));
        assert!(!under_root(".git/hooks/pre-commit.ts"));
    }

    #[test]
    fn sources_that_share_a_prefix_are_still_watched() {
        assert!(under_root("bindings.ts"));
        assert!(under_root("binary/loader.ts"));
        assert!(under_root("node_modules_helper/x.ts"));
        assert!(under_root("src/game.ts"));
    }

    #[test]
    fn only_code_and_models_are_relevant() {
        assert!(under_root("scene.composite"));
        assert!(under_root("assets/tree.glb"));
        assert!(under_root("assets/tree.GLTF"));
        assert!(!under_root("src/tex.png"));
        assert!(!under_root("README.md"));
    }
}
