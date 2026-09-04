use crate::build::{self, BuildOptions};
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

/// A batch closes after `QUIET` without an event (a save is several events a
/// few ms apart), and no later than `DEBOUNCE` after the first.
const QUIET: Duration = Duration::from_millis(20);
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
            let mut quiet_until = tokio::time::Instant::now() + QUIET;
            loop {
                let until = deadline.min(quiet_until);
                match tokio::time::timeout_at(until, self.rx.recv()).await {
                    Ok(Some(p)) => {
                        if is_relevant(&self.root, &p) {
                            batch.push(p);
                        }
                        quiet_until = tokio::time::Instant::now() + QUIET;
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
    if first.is_some_and(|f| f.starts_with('.') || matches!(f, "node_modules" | "bin")) {
        return false;
    }
    is_model(path)
        || matches!(
            path.extension().and_then(|e| e.to_str()).unwrap_or(""),
            "ts" | "tsx" | "js" | "jsx" | "composite"
        )
}

pub fn is_model(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("glb") || e.eq_ignore_ascii_case("gltf"))
}

/// Splits a batch into (model, removed) pairs and everything else. Deletion is
/// judged by what is on disk, not by the notify event kind: an atomic save and a
/// trash-style delete both end with the path present/absent, which is all the
/// explorer needs to pick UMT_CHANGE vs UMT_REMOVE.
fn partition_batch(paths: Vec<PathBuf>) -> (Vec<(PathBuf, bool)>, Vec<PathBuf>) {
    let (mut models, code): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| is_model(p));
    models.sort();
    models.dedup();
    let models = models
        .into_iter()
        .map(|p| {
            let removed = !p.exists();
            (p, removed)
        })
        .collect();
    (models, code)
}

struct SplitState {
    sdk_opts: EsbuildOptions,
    registry: Vec<&'static str>,
    /// The `~sdk/script-utils` module the chunk was last built with: a composite
    /// edit can flip it between the stub and the real runtime with no registry
    /// key changing.
    script_utils: String,
    generated_dir: PathBuf,
}

pub struct WatchSession {
    project: Project,
    es_opts: EsbuildOptions,
    ignore_composite: bool,
    custom_entry_point: bool,
    split: SplitState,
    /// `Some` when the toolchain ships prebuilt chunks: the SDK chunk is copied
    /// rather than bundled, and the smart-item chunk is a separate file.
    prebuilt: Option<crate::prebuilt::Prebuilt>,
    loader: split::Loader,
    typecheck: build::BackgroundCheck,
    type_checking: bool,
}

impl WatchSession {
    pub async fn create(
        project: Project,
        opts: &BuildOptions,
        initial_build: bool,
        steps: &mut ux::Steps,
    ) -> Result<Self> {
        let mut staged = build::stage(&project, opts, &project.root)?;
        staged.loader.write()?;
        if initial_build {
            steps.done(format!(
                "Loader stub saved {}",
                ux::rel_to(&project.root, &staged.loader.outfile)
            ));
            let smart = build::emit_chunks(&project, &staged, &project.root, steps).await?;
            if smart {
                staged.loader.smart_installed = true;
                staged.loader.write()?;
            }
        }
        let build::Staged {
            generated,
            prebuilt,
            sdk_opts,
            scene_opts,
            loader,
        } = staged;
        let registry = split::registry_keys(&project);
        let script_utils = read_script_utils(&generated.dir);
        let mut session = Self {
            project,
            es_opts: scene_opts,
            ignore_composite: opts.ignore_composite,
            custom_entry_point: opts.custom_entry_point,
            split: SplitState {
                sdk_opts,
                registry,
                script_utils,
                generated_dir: generated.dir,
            },
            prebuilt,
            loader,
            typecheck: build::BackgroundCheck::default(),
            type_checking: !opts.skip_type_check,
        };
        if session.type_checking && initial_build {
            session.typecheck.restart(session.project.clone());
        }
        Ok(session)
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    fn rewrite_loader_stub(&self) {
        if let Err(e) = self.loader.write() {
            ux::report_watch(&e);
        }
    }

    pub async fn run(mut self, mut fs: FsWatcher, notify: impl Fn(ReloadEvent)) -> Result<()> {
        while let Some(batch) = fs.next_batch().await {
            let (models, paths) = partition_batch(batch);
            for (model, removed) in &models {
                let verb = if *removed { "removed" } else { "update" };
                ux::note_clocked(format!(
                    "\u{21bb} model {verb} {}",
                    ux::rel_to(&self.project.root, model)
                ));
            }
            for (path, removed) in models {
                notify(ReloadEvent::Model { path, removed });
            }
            if paths.is_empty() {
                continue;
            }
            let started = Instant::now();
            let composites_changed = match regenerate_composites(
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
                Ok(Some(new_max)) => {
                    if new_max != self.loader.max_composite_entity {
                        self.loader.max_composite_entity = new_max;
                        self.rewrite_loader_stub();
                    }
                    true
                }
                Ok(None) => false,
            };
            if self.prebuilt.is_none() {
                refresh_sdk_chunk_cli(&self.project, &mut self.split, composites_changed).await;
            }
            if let Err(e) = esbuild::bundle(&self.project, &self.es_opts).await {
                ux::report_watch(&e);
                continue;
            }
            tracing::info!(
                "rebuilt {} in {}",
                self.es_opts.outfile.display(),
                ux::fmt_elapsed(started.elapsed())
            );
            match build::install_smart_chunk(
                &self.project,
                self.prebuilt.as_ref(),
                &self.project.root,
                &self.loader.paths.scene,
                &self.loader.paths.smart,
            ) {
                Ok(now) if now != self.loader.smart_installed => {
                    self.loader.smart_installed = now;
                    self.rewrite_loader_stub();
                }
                Ok(_) => {}
                Err(e) => ux::report_watch(&e),
            }
            ux::note_clocked(format!(
                "\u{21bb} rebuilt {} ({})",
                ux::rel_to(&self.project.root, &self.es_opts.outfile),
                ux::fmt_elapsed_tinted(started.elapsed(), ux::RESTORE_DIM)
            ));
            notify(ReloadEvent::Scene);
            if self.type_checking {
                self.typecheck.restart(self.project.clone());
            }
        }
        Ok(())
    }
}

fn read_script_utils(generated_dir: &Path) -> String {
    std::fs::read_to_string(generated_dir.join("script-utils.js")).unwrap_or_default()
}

fn watch_regen_error(e: anyhow::Error, what: &str) -> anyhow::Error {
    UserError::new(
        what.to_string(),
        TrySteps::one("fix the file named below, then save any file to retry"),
    )
    .why(format!("{e:#}"))
    .into()
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
    if !ignore_composite {
        match crate::entity_names::write_if_changed(&project.root) {
            Ok(Some(n)) => tracing::info!(
                "composites changed, regenerated {} ({n} name(s))",
                crate::entity_names::OUTPUT_PATH
            ),
            Ok(None) => {}
            Err(e) => tracing::warn!("could not write {}: {e}", crate::entity_names::OUTPUT_PATH),
        }
    }
    if let Err(e) = crate::data_layer::regenerate_main_crdt(&project.root, ignore_composite).await {
        ux::report_watch(&e);
    }
    Ok(Some(generated.max_composite_entity))
}

async fn refresh_sdk_chunk_cli(project: &Project, sp: &mut SplitState, composites_changed: bool) {
    let keys = split::registry_keys(project);
    let script_utils = match composites_changed {
        true => read_script_utils(&sp.generated_dir),
        false => sp.script_utils.clone(),
    };
    if keys == sp.registry && script_utils == sp.script_utils {
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
            sp.script_utils = script_utils;
            tracing::info!(
                "sdk registry changed, rebuilt {}",
                sp.sdk_opts.outfile.display()
            );
            ux::note_clocked(format!(
                "\u{21bb} rebuilt {} (sdk registry changed)",
                ux::rel_to(&project.root, &sp.sdk_opts.outfile)
            ));
        }
        Err(e) => ux::report_watch(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::{is_relevant, partition_batch};
    use std::path::Path;

    #[test]
    fn partition_batch_flags_missing_models_as_removed() {
        let dir = crate::scene::Tmp::new("partition");
        let present = dir.0.join("tree.glb");
        std::fs::write(&present, b"glb").unwrap();
        let gone = dir.0.join("old.gltf");
        let (models, code) = partition_batch(vec![
            present.clone(),
            gone.clone(),
            present.clone(),
            dir.0.join("src/game.ts"),
        ]);
        assert_eq!(code, vec![dir.0.join("src/game.ts")]);
        assert_eq!(models, vec![(gone, true), (present, false)]);
    }

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
        assert!(!under_root(".dcl-optimized-assets/out/b64-x/mac/model.glb"));
        assert!(!under_root(
            ".dcl-optimized-assets/cache/content/deadbeef.gltf"
        ));
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
