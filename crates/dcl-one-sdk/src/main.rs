use anyhow::Result;
use clap::{Parser, Subcommand};
use dcl_one_sdk::{
    build, context_files, deploy, init, pack, scene, start, ux, watch, workspace, world,
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "dcl-one-sdk",
    version,
    about = "Binary-compatible Rust replacement for @dcl/sdk-commands (build, start, deploy)"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "Show detailed logs and full error chains (RUST_LOG also enables this)"
    )]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Scaffold a new scene or smart wearable project")]
    Init {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        #[arg(long, value_enum)]
        project: Option<init::ProjectKind>,
        #[arg(short = 'y', long)]
        yes: bool,
        #[arg(
            long,
            help = "Only install the vendored node_modules into an existing project; scaffold nothing"
        )]
        node_modules_only: bool,
    },
    #[command(about = "Download the official SDK7 AI context files into dclcontext/")]
    GetContextFiles {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    #[command(about = "Type-check and bundle the scene into bin/index.js")]
    Build {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        #[arg(short = 'p', long)]
        production: bool,
        #[arg(short = 'w', long)]
        watch: bool,
        #[arg(long = "ignoreComposite", visible_alias = "ignore-composite")]
        ignore_composite: bool,
        #[arg(long = "customEntryPoint", visible_alias = "custom-entry-point")]
        custom_entry_point: bool,
        #[arg(long)]
        skip_install: bool,
        #[arg(long)]
        skip_type_check: bool,
    },
    #[command(about = "Build the scene and serve a live preview with hot reload")]
    Start {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        #[arg(short = 'p', long, default_value_t = 8000)]
        port: u16,
        #[arg(long)]
        skip_build: bool,
        #[arg(long)]
        skip_install: bool,
        #[arg(short = 'w', long)]
        no_watch: bool,
        #[arg(short = 'b', long)]
        no_browser: bool,
        #[arg(long)]
        ci: bool,
        #[arg(long)]
        data_layer: bool,
        #[arg(long = "ignoreComposite", visible_alias = "ignore-composite")]
        ignore_composite: bool,
        #[arg(long)]
        offline_comms: bool,
        #[arg(long, hide = true)]
        mini_comms: bool,
        #[arg(short = 'm', long)]
        mobile: bool,
        #[arg(
            long,
            help = "Do not run the abgen asset-bundle sidecar. By default start resolves abgen from ABGEN_BIN, then the copy embedded in release binaries, then the scene's @dcl/abgen npm package, then PATH; previews continue with a hint when none is found"
        )]
        no_asset_bundles: bool,
        #[arg(long, value_name = "WSS_URL|help")]
        tunnel: Option<String>,
        #[arg(
            long,
            help = "Auth token for the --tunnel service; prefer --tunnel-token-file or DCL_ONE_SDK_TUNNEL_TOKEN (a flag value is visible in ps and shell history)"
        )]
        tunnel_token: Option<String>,
        #[arg(
            long,
            value_name = "PATH",
            help = "Read the --tunnel auth token from a file (wins over DCL_ONE_SDK_TUNNEL_TOKEN; --tunnel-token wins over both)"
        )]
        tunnel_token_file: Option<PathBuf>,
    },
    #[command(about = "Sign and publish the scene to a catalyst or worlds content server")]
    Deploy {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        #[arg(short = 't', long)]
        target: Option<String>,
        #[arg(long)]
        target_content: Option<String>,
        #[arg(long)]
        sign_key: Option<PathBuf>,
        #[arg(long)]
        skip_build: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        timestamp: Option<i64>,
        #[arg(long)]
        entity_out: Option<PathBuf>,
        #[arg(long)]
        multi_scene: bool,
        #[arg(short = 'y', long)]
        yes: bool,
        #[arg(short = 'b', long)]
        no_browser: bool,
        #[arg(long)]
        ci: bool,
        #[arg(short = 'p', long)]
        port: Option<u16>,
    },
    #[command(
        alias = "pack-smart-wearable",
        about = "Build and zip a smart wearable for upload to the builder"
    )]
    Pack {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        #[arg(long)]
        skip_build: bool,
    },
    #[command(about = "Manage a world's settings and permissions on a worlds content server")]
    World {
        #[command(subcommand)]
        command: WorldCommand,
    },
}

#[derive(Subcommand)]
enum WorldCommand {
    #[command(about = "Get or set world metadata (title, spawn, skybox, categories, ...)")]
    Settings {
        #[command(subcommand)]
        command: WorldSettingsCommand,
    },
    #[command(about = "List, grant, or revoke world access permissions")]
    Permissions {
        #[command(subcommand)]
        command: WorldPermissionsCommand,
    },
}

#[derive(Subcommand)]
enum WorldSettingsCommand {
    #[command(about = "Print the current settings of a world")]
    Get {
        name: String,
        #[arg(long)]
        target_content: Option<String>,
    },
    #[command(about = "Update settings fields of a world (signed request)")]
    Set {
        name: String,
        #[arg(long)]
        target_content: Option<String>,
        #[arg(long)]
        sign_key: Option<PathBuf>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        content_rating: Option<String>,
        #[arg(long)]
        spawn_coordinates: Option<String>,
        #[arg(long)]
        skybox_time: Option<String>,
        #[arg(long)]
        single_player: Option<bool>,
        #[arg(long)]
        show_in_places: Option<bool>,
        #[arg(long = "category")]
        categories: Vec<String>,
        #[arg(long)]
        thumbnail: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum WorldPermissionsCommand {
    #[command(about = "Print who holds each permission on a world")]
    List {
        name: String,
        #[arg(long)]
        target_content: Option<String>,
    },
    #[command(about = "Grant a permission on a world to an address (signed request)")]
    Grant {
        name: String,
        permission: String,
        address: String,
        #[arg(long)]
        target_content: Option<String>,
        #[arg(long)]
        sign_key: Option<PathBuf>,
    },
    #[command(about = "Revoke a permission on a world from an address (signed request)")]
    Revoke {
        name: String,
        permission: String,
        address: String,
        #[arg(long)]
        target_content: Option<String>,
        #[arg(long)]
        sign_key: Option<PathBuf>,
    },
}

struct PlainFormat;

impl<S, N> tracing_subscriber::fmt::FormatEvent<S, N> for PlainFormat
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let prefix = match *event.metadata().level() {
            tracing::Level::ERROR => "error: ",
            tracing::Level::WARN => "warning: ",
            _ => "",
        };
        write!(writer, "{prefix}")?;
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

fn init_tracing(verbose: bool) {
    if verbose {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info".into()),
            )
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new("warn"))
            .event_format(PlainFormat)
            .with_writer(std::io::stderr)
            .init();
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let verbose = cli.verbose || std::env::var_os("RUST_LOG").is_some();
    init_tracing(verbose);
    if let Err(e) = run(cli.command).await {
        ux::report(&e, verbose);
        std::process::exit(1);
    }
}

async fn run(command: Command) -> Result<()> {
    match command {
        Command::Init {
            dir,
            project,
            yes,
            node_modules_only,
        } => init::init(&init::InitOptions {
            dir,
            project,
            yes,
            node_modules_only,
        }),
        Command::GetContextFiles { dir } => {
            let api = std::env::var("DCL_ONE_SDK_CONTEXT_API")
                .unwrap_or_else(|_| context_files::DEFAULT_API.to_string());
            context_files::get_context_files(&dir, &api).await
        }
        Command::Build {
            dir,
            production,
            watch,
            ignore_composite,
            custom_entry_point,
            skip_install,
            skip_type_check,
        } => {
            if skip_install {
                ux::note("--skip-install has no effect (dcl-one-sdk never installs packages)");
            }
            let opts = build::BuildOptions {
                dir,
                production,
                ignore_composite,
                custom_entry_point,
                skip_type_check,
            };
            if workspace::member_folders(&opts.dir)?.is_some() {
                let ws = workspace::Workspace::load(&opts.dir)?;
                if watch {
                    return watch_workspace(&ws, &opts).await;
                }
                return build::build_workspace(&ws, &opts).await;
            }
            if watch {
                let project = scene::Project::load(&opts.dir)?;
                let fs = watch::FsWatcher::new(&project.root)?;
                let chunk = 3;
                let tc = if opts.skip_type_check { 0 } else { 1 };
                let mut steps = ux::Steps::new(chunk + tc + 1);
                let session = watch::WatchSession::create(project, &opts, true, &mut steps).await?;
                if opts.skip_type_check {
                    ux::note("type check skipped (--skip-type-check)");
                } else {
                    match build::type_check(session.project()).await {
                        Ok(()) => {
                            tracing::info!("type checking completed without errors");
                            steps.done("Type check passed");
                        }
                        Err(e) => ux::report_watch(&e),
                    }
                }
                steps.done("Watching for changes (ctrl-c to stop)");
                tokio::select! {
                    r = session.run(fs, |_| {}) => r,
                    _ = tokio::signal::ctrl_c() => Ok(()),
                }
            } else {
                build::build(&opts).await.map(|_| ())
            }
        }
        Command::Start {
            dir,
            port,
            skip_build,
            skip_install,
            no_watch,
            no_browser,
            ci,
            data_layer,
            ignore_composite,
            offline_comms,
            mini_comms,
            mobile,
            no_asset_bundles,
            tunnel,
            tunnel_token,
            tunnel_token_file,
        } => {
            if tunnel.as_deref().map(str::trim) == Some("help") {
                println!("{}", dcl_one_sdk::tunnel::tunnel_help());
                return Ok(());
            }
            let tunnel_token = if tunnel.is_some() {
                dcl_one_sdk::tunnel::resolve_token(tunnel_token, tunnel_token_file.as_deref())?
            } else {
                tunnel_token
            };
            if skip_install {
                ux::note("--skip-install has no effect (dcl-one-sdk never installs packages)");
            }
            if no_browser {
                ux::note("--no-browser has no effect (dcl-one-sdk never opens a browser)");
            }
            if ci {
                ux::note("--ci has no effect yet");
            }
            if mini_comms {
                ux::note("--mini-comms has no effect (the built-in ws-room relay is always on)");
            }
            start::start(start::StartOptions {
                dir,
                port,
                skip_build,
                no_watch,
                ignore_composite,
                offline_comms,
                mobile,
                asset_bundles: !no_asset_bundles,
                data_layer,
                tunnel,
                tunnel_token,
            })
            .await
        }
        Command::Deploy {
            dir,
            target,
            target_content,
            sign_key,
            skip_build,
            dry_run,
            timestamp,
            entity_out,
            multi_scene,
            yes,
            no_browser,
            ci,
            port,
        } => {
            deploy::deploy(&deploy::DeployOptions {
                dir,
                target,
                target_content,
                sign_key,
                skip_build,
                dry_run,
                timestamp,
                entity_out,
                multi_scene,
                yes,
                no_browser,
                ci,
                port,
            })
            .await
        }
        Command::Pack { dir, skip_build } => {
            pack::pack(&pack::PackOptions { dir, skip_build }).await
        }
        Command::World { command } => run_world(command).await,
    }
}

async fn run_world(command: WorldCommand) -> Result<()> {
    match command {
        WorldCommand::Settings { command } => match command {
            WorldSettingsCommand::Get {
                name,
                target_content,
            } => world::settings_get(&name, target_content.as_deref()).await,
            WorldSettingsCommand::Set {
                name,
                target_content,
                sign_key,
                title,
                description,
                content_rating,
                spawn_coordinates,
                skybox_time,
                single_player,
                show_in_places,
                categories,
                thumbnail,
            } => {
                world::settings_set(
                    &name,
                    target_content.as_deref(),
                    sign_key.as_deref(),
                    world::SettingsUpdate {
                        title,
                        description,
                        content_rating,
                        spawn_coordinates,
                        skybox_time,
                        single_player,
                        show_in_places,
                        categories,
                        thumbnail,
                    },
                )
                .await
            }
        },
        WorldCommand::Permissions { command } => match command {
            WorldPermissionsCommand::List {
                name,
                target_content,
            } => world::permissions_list(&name, target_content.as_deref()).await,
            WorldPermissionsCommand::Grant {
                name,
                permission,
                address,
                target_content,
                sign_key,
            } => {
                world::permissions_grant(
                    &name,
                    &permission,
                    &address,
                    target_content.as_deref(),
                    sign_key.as_deref(),
                )
                .await
            }
            WorldPermissionsCommand::Revoke {
                name,
                permission,
                address,
                target_content,
                sign_key,
            } => {
                world::permissions_revoke(
                    &name,
                    &permission,
                    &address,
                    target_content.as_deref(),
                    sign_key.as_deref(),
                )
                .await
            }
        },
    }
}

async fn watch_workspace(ws: &workspace::Workspace, opts: &build::BuildOptions) -> Result<()> {
    let mut runners = Vec::new();
    for (i, project) in ws.projects.iter().enumerate() {
        if let Some(header) = ws.member_header(i) {
            ux::note(header);
        }
        let member = build::member_options(opts, project);
        let chunk = 3;
        let tc = if member.skip_type_check { 0 } else { 1 };
        let mut steps = ux::Steps::new(chunk + tc);
        let fs = watch::FsWatcher::new(&project.root)?;
        let session =
            watch::WatchSession::create(project.clone(), &member, true, &mut steps).await?;
        if member.skip_type_check {
            ux::note("type check skipped (--skip-type-check)");
        } else {
            match build::type_check(session.project()).await {
                Ok(()) => {
                    tracing::info!("type checking completed without errors");
                    steps.done("Type check passed");
                }
                Err(e) => ux::report_watch(&e),
            }
        }
        runners.push((session, fs));
    }
    ux::note("Watching for changes (ctrl-c to stop)");
    let mut set = tokio::task::JoinSet::new();
    for (session, fs) in runners {
        set.spawn(session.run(fs, |_| {}));
    }
    tokio::select! {
        joined = set.join_next() => match joined {
            Some(Ok(r)) => r,
            Some(Err(e)) => Err(e.into()),
            None => Ok(()),
        },
        _ = tokio::signal::ctrl_c() => Ok(()),
    }
}
