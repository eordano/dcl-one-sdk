use crate::build::BuildOptions;
use crate::entrypoint;
use crate::esbuild::{self, Backend, EsbuildOptions};
use crate::esbuild_service::{BuildContext, BuildStatus, EsbuildService};
use crate::live_reload::ReloadEvent;
use crate::scene::Project;
use crate::split;
use crate::ux::{self, TrySteps, UserError};
use anyhow::{Context, Result};
use notify::{RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
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

enum Mode {
    Service {
        service: EsbuildService,
        ctx: BuildContext,
        sdk_ctx: Option<BuildContext>,
    },
    CliWatch {
        _child: tokio::process::Child,
        done_rx: mpsc::UnboundedReceiver<()>,
    },
    Rebundle,
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
    split: Option<SplitState>,
    mode: Mode,
}

impl WatchSession {
    pub async fn create(
        project: Project,
        opts: &BuildOptions,
        initial_build: bool,
        steps: &mut ux::Steps,
    ) -> Result<Self> {
        if opts.split_sdk {
            return Self::create_split(project, opts, initial_build, steps).await;
        }
        let outfile = project.root.join(project.main_output()?);
        let generated = entrypoint::generate(
            &project,
            opts.ignore_composite,
            opts.custom_entry_point,
            false,
        )?;
        split::clear_marker(&generated.dir);
        let mut aliases = esbuild::resolve_aliases(&project)?;
        aliases.push((
            "~sdk/all-composites".to_string(),
            generated.dir.join("all-composites.js"),
        ));
        aliases.push((
            "~sdk/script-utils".to_string(),
            generated.dir.join("script-utils.js"),
        ));
        let es_opts = EsbuildOptions {
            backend: opts.backend,
            production: opts.production,
            entrypoint: generated.entrypoint,
            outfile,
            tsconfig: project.tsconfig()?,
            aliases,
            externals: vec![],
        };
        let mode = if opts.backend == Backend::Rolldown {
            if initial_build {
                let started = Instant::now();
                esbuild::bundle(&project, &es_opts).await?;
                tracing::info!("bundle saved {}", es_opts.outfile.display());
                steps.done(format!(
                    "Bundle saved {} ({})",
                    ux::rel_to(&project.root, &es_opts.outfile),
                    ux::fmt_elapsed(started.elapsed())
                ));
            }
            Mode::Rebundle
        } else if std::env::var("DCL_ONE_SDK_NO_SERVICE").is_ok_and(|v| v == "1") {
            cli_watch(&project, &es_opts, initial_build.then_some(&mut *steps)).await?
        } else {
            match service_context(&project, &es_opts, initial_build, &mut *steps).await {
                Ok(mode) => mode,
                Err(e) => {
                    tracing::warn!(
                        "esbuild service unavailable for watch ({e:#}); falling back to esbuild --watch"
                    );
                    cli_watch(&project, &es_opts, initial_build.then_some(&mut *steps)).await?
                }
            }
        };
        Ok(Self {
            project,
            es_opts,
            ignore_composite: opts.ignore_composite,
            custom_entry_point: opts.custom_entry_point,
            split: None,
            mode,
        })
    }

    async fn create_split(
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
        split::write_loader_stub(&outfile, &sdk_rel, &scene_rel)?;
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
            backend: opts.backend,
            production: opts.production,
            entrypoint: generated.dir.join("sdk-runtime-entry.js"),
            outfile: project.root.join(&sdk_rel),
            tsconfig: project.tsconfig()?,
            aliases: sdk_aliases,
            externals: vec![],
        };
        let scene_opts = EsbuildOptions {
            backend: opts.backend,
            production: opts.production,
            entrypoint: generated.entrypoint,
            outfile: project.root.join(&scene_rel),
            tsconfig: project.tsconfig()?,
            aliases: vec![],
            externals: split::scene_externals(&project),
        };
        let mode = if opts.backend == Backend::Rolldown {
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
            Mode::Rebundle
        } else if std::env::var("DCL_ONE_SDK_NO_SERVICE").is_ok_and(|v| v == "1") {
            split_cli_watch(
                &project,
                &sdk_opts,
                &scene_opts,
                initial_build,
                initial_build.then_some(&mut *steps),
            )
            .await?
        } else {
            match split_service_context(
                &project,
                &sdk_opts,
                &scene_opts,
                initial_build,
                &mut *steps,
            )
            .await
            {
                Ok(mode) => mode,
                Err(e) => {
                    tracing::warn!(
                        "esbuild service unavailable for split watch ({e:#}); falling back to esbuild --watch"
                    );
                    split_cli_watch(
                        &project,
                        &sdk_opts,
                        &scene_opts,
                        initial_build,
                        initial_build.then_some(&mut *steps),
                    )
                    .await?
                }
            }
        };
        let registry = split::registry_keys(&project);
        Ok(Self {
            project,
            es_opts: scene_opts,
            ignore_composite: opts.ignore_composite,
            custom_entry_point: opts.custom_entry_point,
            split: Some(SplitState {
                sdk_opts,
                registry,
                generated_dir: generated.dir,
            }),
            mode,
        })
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub async fn run(mut self, mut fs: FsWatcher, notify: impl Fn(ReloadEvent)) -> Result<()> {
        loop {
            let mut fall_back = false;
            match &mut self.mode {
                Mode::Service {
                    service,
                    ctx,
                    sdk_ctx,
                } => {
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
                    if let Err(e) = regenerate_composites(
                        &self.project,
                        self.ignore_composite,
                        self.custom_entry_point,
                        self.split.is_some(),
                        &paths,
                    )
                    .await
                    {
                        ux::report_watch(&watch_regen_error(
                            e,
                            "composite rebuild failed \u{2014} watching continues",
                        ));
                        continue;
                    }
                    if let (Some(sp), Some(sdk)) = (&mut self.split, sdk_ctx.as_mut()) {
                        let keys = split::registry_keys(&self.project);
                        if keys != sp.registry {
                            if let Err(e) = split::write_generated(&self.project, &sp.generated_dir)
                            {
                                ux::report_watch(&watch_regen_error(
                                    e,
                                    "sdk runtime entry rebuild failed \u{2014} watching continues",
                                ));
                                continue;
                            }
                            match service.rebuild(sdk).await {
                                Ok(BuildStatus::Success) => {
                                    sp.registry = keys;
                                    tracing::info!(
                                        "sdk registry changed, rebuilt {}",
                                        sp.sdk_opts.outfile.display()
                                    );
                                }
                                Ok(BuildStatus::Failed(msg)) => {
                                    ux::report_watch(&ux::bundle_failed(&msg));
                                    continue;
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "esbuild service failed mid-watch ({e:#}); falling back to esbuild --watch"
                                    );
                                    fall_back = true;
                                }
                            }
                        }
                    }
                    if !fall_back {
                        match service.rebuild(ctx).await {
                            Ok(BuildStatus::Success) => {
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
                            Ok(BuildStatus::Failed(msg)) => {
                                ux::report_watch(&ux::bundle_failed(&msg))
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "esbuild service failed mid-watch ({e:#}); falling back to esbuild --watch"
                                );
                                fall_back = true;
                            }
                        }
                    }
                }
                Mode::Rebundle => {
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
                    if let Err(e) = regenerate_composites(
                        &self.project,
                        self.ignore_composite,
                        self.custom_entry_point,
                        self.split.is_some(),
                        &paths,
                    )
                    .await
                    {
                        ux::report_watch(&watch_regen_error(
                            e,
                            "composite rebuild failed \u{2014} watching continues",
                        ));
                        continue;
                    }
                    if let Some(sp) = &mut self.split {
                        refresh_sdk_chunk_cli(&self.project, sp).await;
                    }
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
                Mode::CliWatch { done_rx, .. } => {
                    tokio::select! {
                        batch = fs.next_batch() => {
                            let Some(batch) = batch else {
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
                            if let Err(e) = regenerate_composites(
                                &self.project,
                                self.ignore_composite,
                                self.custom_entry_point,
                                self.split.is_some(),
                                &paths,
                            )
                            .await
                            {
                                ux::report_watch(&watch_regen_error(e, "composite rebuild failed \u{2014} watching continues"));
                            }
                            if let Some(sp) = &mut self.split {
                                refresh_sdk_chunk_cli(&self.project, sp).await;
                            }
                        }
                        done = done_rx.recv() => {
                            if done.is_none() {
                                return Err(UserError::new(
                                    "the file watcher stopped (esbuild --watch exited)",
                                    TrySteps::one("restart dcl-one-sdk start")
                                        .and("re-run with --verbose to capture why it exited"),
                                )
                                .into());
                            }
                            tracing::info!("rebuilt {} (esbuild --watch)", self.es_opts.outfile.display());
                            ux::note(format!(
                                "\u{21bb} rebuilt {} (esbuild --watch)",
                                ux::rel_to(&self.project.root, &self.es_opts.outfile)
                            ));
                            notify(ReloadEvent::Scene);
                        }
                    }
                }
            }
            if fall_back {
                self.mode = cli_watch(&self.project, &self.es_opts, None).await?;
                notify(ReloadEvent::Scene);
            }
        }
        if let Mode::Service {
            service,
            ctx,
            sdk_ctx,
        } = self.mode
        {
            if let Some(sdk) = sdk_ctx {
                let _ = service.dispose(sdk).await;
            }
            let _ = service.dispose(ctx).await;
            service.shutdown().await;
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
    split: bool,
    paths: &[PathBuf],
) -> Result<()> {
    let touched = paths
        .iter()
        .any(|p| p.extension().and_then(|e| e.to_str()) == Some("composite"));
    if !touched {
        return Ok(());
    }
    entrypoint::generate(project, ignore_composite, custom_entry_point, split)?;
    tracing::info!("composites changed, regenerated all-composites.js");
    crate::data_layer::regenerate_main_crdt(&project.root, ignore_composite).await;
    Ok(())
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

async fn service_context(
    project: &Project,
    es_opts: &EsbuildOptions,
    initial_build: bool,
    steps: &mut ux::Steps,
) -> Result<Mode> {
    let bin = esbuild::locate(project)?;
    let mut service = EsbuildService::spawn(&bin, &project.root).await?;
    match service.create_context(es_opts, &project.root).await {
        Ok((mut ctx, BuildStatus::Success)) => {
            if initial_build {
                let started = Instant::now();
                match service.rebuild(&mut ctx).await {
                    Ok(BuildStatus::Success) => {
                        tracing::info!(
                            "bundle saved {} in {:.0?}",
                            es_opts.outfile.display(),
                            started.elapsed()
                        );
                        steps.done(format!(
                            "Bundle saved {} ({})",
                            ux::rel_to(&project.root, &es_opts.outfile),
                            ux::fmt_elapsed(started.elapsed())
                        ));
                    }
                    Ok(BuildStatus::Failed(msg)) => {
                        tracing::warn!("initial build failed (still watching)");
                        ux::report_watch(&ux::bundle_failed(&msg));
                    }
                    Err(e) => {
                        let _ = service.dispose(ctx).await;
                        service.shutdown().await;
                        return Err(e);
                    }
                }
            }
            Ok(Mode::Service {
                service,
                ctx,
                sdk_ctx: None,
            })
        }
        Ok((ctx, BuildStatus::Failed(msg))) => {
            let _ = service.dispose(ctx).await;
            service.shutdown().await;
            Err(ux::bundle_failed(&msg))
        }
        Err(e) => {
            service.shutdown().await;
            Err(e)
        }
    }
}

async fn split_service_context(
    project: &Project,
    sdk_opts: &EsbuildOptions,
    scene_opts: &EsbuildOptions,
    initial_build: bool,
    steps: &mut ux::Steps,
) -> Result<Mode> {
    let bin = esbuild::locate(project)?;
    let mut service = EsbuildService::spawn(&bin, &project.root).await?;
    let mut sdk_ctx = match service.create_context(sdk_opts, &project.root).await {
        Ok((ctx, BuildStatus::Success)) => ctx,
        Ok((ctx, BuildStatus::Failed(msg))) => {
            let _ = service.dispose(ctx).await;
            service.shutdown().await;
            return Err(ux::bundle_failed(&msg));
        }
        Err(e) => {
            service.shutdown().await;
            return Err(e);
        }
    };
    let mut scene_ctx = match service.create_context(scene_opts, &project.root).await {
        Ok((ctx, BuildStatus::Success)) => ctx,
        Ok((ctx, BuildStatus::Failed(msg))) => {
            let _ = service.dispose(ctx).await;
            let _ = service.dispose(sdk_ctx).await;
            service.shutdown().await;
            return Err(ux::bundle_failed(&msg));
        }
        Err(e) => {
            let _ = service.dispose(sdk_ctx).await;
            service.shutdown().await;
            return Err(e);
        }
    };
    if initial_build {
        let started = Instant::now();
        match service.rebuild(&mut sdk_ctx).await {
            Ok(BuildStatus::Success) => {
                tracing::info!(
                    "sdk chunk saved {} in {:.0?}",
                    sdk_opts.outfile.display(),
                    started.elapsed()
                );
                steps.done(format!(
                    "SDK chunk saved {} ({})",
                    ux::rel_to(&project.root, &sdk_opts.outfile),
                    ux::fmt_elapsed(started.elapsed())
                ));
            }
            Ok(BuildStatus::Failed(msg)) => {
                tracing::warn!("initial sdk chunk build failed (still watching)");
                ux::report_watch(&ux::bundle_failed(&msg));
            }
            Err(e) => {
                let _ = service.dispose(sdk_ctx).await;
                let _ = service.dispose(scene_ctx).await;
                service.shutdown().await;
                return Err(e);
            }
        }
        let started = Instant::now();
        match service.rebuild(&mut scene_ctx).await {
            Ok(BuildStatus::Success) => {
                tracing::info!(
                    "scene chunk saved {} in {:.0?}",
                    scene_opts.outfile.display(),
                    started.elapsed()
                );
                steps.done(format!(
                    "Scene chunk saved {} ({})",
                    ux::rel_to(&project.root, &scene_opts.outfile),
                    ux::fmt_elapsed(started.elapsed())
                ));
            }
            Ok(BuildStatus::Failed(msg)) => {
                tracing::warn!("initial scene chunk build failed (still watching)");
                ux::report_watch(&ux::bundle_failed(&msg));
            }
            Err(e) => {
                let _ = service.dispose(sdk_ctx).await;
                let _ = service.dispose(scene_ctx).await;
                service.shutdown().await;
                return Err(e);
            }
        }
    }
    Ok(Mode::Service {
        service,
        ctx: scene_ctx,
        sdk_ctx: Some(sdk_ctx),
    })
}

async fn split_cli_watch(
    project: &Project,
    sdk_opts: &EsbuildOptions,
    scene_opts: &EsbuildOptions,
    initial_build: bool,
    mut steps: Option<&mut ux::Steps>,
) -> Result<Mode> {
    if initial_build || !sdk_opts.outfile.exists() {
        let started = Instant::now();
        esbuild::run(project, sdk_opts).await?;
        tracing::info!("sdk chunk saved {}", sdk_opts.outfile.display());
        if let Some(steps) = steps.as_deref_mut() {
            steps.done(format!(
                "SDK chunk saved {} ({})",
                ux::rel_to(&project.root, &sdk_opts.outfile),
                ux::fmt_elapsed(started.elapsed())
            ));
        }
    }
    cli_watch(project, scene_opts, steps).await
}

async fn cli_watch(
    project: &Project,
    es_opts: &EsbuildOptions,
    steps: Option<&mut ux::Steps>,
) -> Result<Mode> {
    let bin = esbuild::locate(project)?;
    let mut args = esbuild::args(es_opts);
    args.push("--watch".into());
    let started = Instant::now();
    let mut child = tokio::process::Command::new(&bin)
        .args(&args)
        .current_dir(&project.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| esbuild::spawn_error(&bin, e))?;
    let stderr = child
        .stderr
        .take()
        .context("esbuild --watch stderr missing")?;
    let (tx, mut done_rx) = mpsc::unbounded_channel::<()>();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            eprintln!("{line}");
            if line.starts_with("[watch] build finished") {
                let _ = tx.send(());
            }
        }
    });
    match tokio::time::timeout(Duration::from_secs(60), done_rx.recv()).await {
        Ok(Some(())) => {
            if let Some(steps) = steps {
                steps.done(format!(
                    "Bundle saved {} ({})",
                    ux::rel_to(&project.root, &es_opts.outfile),
                    ux::fmt_elapsed(started.elapsed())
                ));
            }
        }
        Ok(None) => {
            return Err(UserError::new(
                "the fallback esbuild watcher did not produce a first build",
                TrySteps::one("re-run with --verbose and check the esbuild output above")
                    .and("run dcl-one-sdk build to see the full error"),
            )
            .why("esbuild --watch exited before its first build")
            .into())
        }
        Err(_) => {
            return Err(UserError::new(
                "the fallback esbuild watcher did not produce a first build",
                TrySteps::one("re-run with --verbose and check the esbuild output above")
                    .and("run dcl-one-sdk build to see the full error"),
            )
            .why("esbuild --watch first build timed out after 60s")
            .into())
        }
    }
    Ok(Mode::CliWatch {
        _child: child,
        done_rx,
    })
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
