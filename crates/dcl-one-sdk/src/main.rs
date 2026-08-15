use anyhow::Result;
use clap::{Args, Parser, Subcommand};
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
        #[arg(long, default_value = ".", help = "Folder to scaffold into")]
        dir: PathBuf,
        #[arg(long, value_enum, help = "What to scaffold (default: scene)")]
        project: Option<init::ProjectKind>,
        #[arg(short = 'y', long, help = "Scaffold even if the folder is not empty")]
        yes: bool,
        #[arg(
            long,
            help = "Only install the vendored node_modules into an existing project; scaffold nothing"
        )]
        node_modules_only: bool,
    },
    #[command(
        about = "Install the bundled migrate-smart-items-to-code skill into .claude/skills/ and download the official SDK7 AI context files into dclcontext/"
    )]
    GetContextFiles {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        #[arg(
            long,
            help = "Only write the bundled skill; skip the GitHub ai-sdk-context download"
        )]
        offline: bool,
    },
    #[command(about = "Type-check and bundle the scene into bin/index.js")]
    Build {
        #[arg(long, default_value = ".", help = "Project folder to build")]
        dir: PathBuf,
        #[arg(
            short = 'p',
            long,
            help = "Minify and drop dev-only checks, as deploy does"
        )]
        production: bool,
        #[arg(short = 'w', long, help = "Rebuild on every change instead of exiting")]
        watch: bool,
        #[arg(
            long = "ignoreComposite",
            visible_alias = "ignore-composite",
            help = "Leave main.crdt alone instead of regenerating it from composites"
        )]
        ignore_composite: bool,
        #[arg(
            long = "customEntryPoint",
            visible_alias = "custom-entry-point",
            help = "Bundle scene.json's main verbatim instead of generating the loader stub"
        )]
        custom_entry_point: bool,
        #[arg(
            long,
            help = "Do not restore missing node_modules from the vendored SDK"
        )]
        skip_install: bool,
        #[arg(
            long,
            help = "Bundle without type checking (the bundle is written either way)"
        )]
        skip_type_check: bool,
    },
    #[command(about = "Build the scene and serve a live preview with hot reload")]
    Start {
        #[arg(long, default_value = ".", help = "Project folder to preview")]
        dir: PathBuf,
        #[arg(
            short = 'p',
            long,
            help = "Port to serve on; without it, 8000 or the next free port"
        )]
        port: Option<u16>,
        #[arg(long, help = "Serve the existing bin/ as-is instead of building first")]
        skip_build: bool,
        #[arg(
            long,
            help = "Do not type check; checking runs beside the watch loop and never delays a reload"
        )]
        skip_type_check: bool,
        #[arg(
            long,
            help = "Do not restore missing node_modules from the vendored SDK"
        )]
        skip_install: bool,
        #[arg(short = 'w', long, help = "Serve once and stop watching for changes")]
        no_watch: bool,
        #[arg(
            short = 'b',
            long,
            help = "Accepted for compatibility; this CLI never opens a browser"
        )]
        no_browser: bool,
        #[arg(long, help = "Non-interactive: no prompts, no TTY-only output")]
        ci: bool,
        #[arg(
            long,
            help = "Expose the Creator Hub data layer so the inspector can edit this scene live"
        )]
        data_layer: bool,
        #[arg(
            long = "ignoreComposite",
            visible_alias = "ignore-composite",
            help = "Leave main.crdt alone instead of regenerating it from composites"
        )]
        ignore_composite: bool,
        #[arg(
            long,
            help = "Serve comms locally so the preview needs no comms service"
        )]
        offline_comms: bool,
        #[arg(long, hide = true)]
        mini_comms: bool,
        #[arg(long = "multi-instance", hide = true)]
        multi_instance: bool,
        #[arg(long = "no-client", hide = true)]
        no_client: bool,
        #[arg(
            short = 'm',
            long,
            help = "Show a QR code and LAN URL for opening the preview on a phone"
        )]
        mobile: bool,
        #[arg(
            long,
            help = "Do not run the abgen asset-bundle sidecar. It is on by default and needs no install \u{2014} every dcl-one-sdk binary embeds abgen (ABGEN_BIN runs a different one). Upstream sdk-commands has no sidecar, so this is how to get its unoptimized preview"
        )]
        no_asset_bundles: bool,
        #[arg(
            long = "asset-bundles",
            conflicts_with = "no_asset_bundles",
            help = "Let the desktop Explorer convert asset bundles itself: forwards local-ab=true in the deep link and skips the local abgen sidecar"
        )]
        asset_bundles: bool,
        #[arg(
            long,
            help = "Enable the MCP server in the Explorer (forwarded into the desktop deep link)"
        )]
        mcp: bool,
        #[arg(
            long = "mcp-port",
            value_name = "PORT",
            help = "Port for the Explorer's MCP server (forwarded into the desktop deep link)"
        )]
        mcp_port: Option<u16>,
        #[arg(
            last = true,
            value_name = "EXPLORER_PARAMS",
            help = "Everything after a standalone -- is forwarded verbatim into the desktop Explorer deep link as query params: --key=value, --key value, and bare --key (becomes key=true)"
        )]
        explorer_params: Vec<String>,
        #[arg(
            long,
            value_name = "WSS_URL|help",
            help = "Expose this preview publicly through a tunnel service; pass 'help' for setup"
        )]
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
        #[arg(long, default_value = ".", help = "Project folder to deploy")]
        dir: PathBuf,
        #[arg(
            short = 't',
            long,
            help = "Catalyst to publish to; its /about is read to find the content server"
        )]
        target: Option<String>,
        #[arg(
            long,
            help = "Content server to publish to directly, bypassing catalyst discovery"
        )]
        target_content: Option<String>,
        #[arg(
            long,
            help = "Sign headlessly with this private-key file instead of opening a wallet (env: DCL_PRIVATE_KEY; this flag wins)"
        )]
        sign_key: Option<PathBuf>,
        #[arg(long, help = "Publish the existing bin/ as-is instead of rebuilding")]
        skip_build: bool,
        #[arg(
            long,
            help = "Pack and hash the entity, print what would be published, and touch no network"
        )]
        dry_run: bool,
        #[arg(
            long,
            help = "Pin the entity timestamp (unix ms) so the same input yields the same entity id"
        )]
        timestamp: Option<i64>,
        #[arg(long, help = "Also write the entity JSON to this path")]
        entity_out: Option<PathBuf>,
        #[arg(long, help = "Deploy every scene in the workspace, not just one")]
        multi_scene: bool,
        #[arg(
            short = 'y',
            long,
            help = "Answer prompts yes, including consent to publish to the public network"
        )]
        yes: bool,
        #[arg(short = 'b', long, help = "Do not open a browser for wallet signing")]
        no_browser: bool,
        #[arg(
            long,
            help = "Non-interactive: never open a browser, and refuse a public deploy unless --yes is given"
        )]
        ci: bool,
        #[arg(
            short = 'p',
            long,
            help = "Port for the local signing page (default: loopback, ephemeral)"
        )]
        port: Option<u16>,
    },
    #[command(
        about = "Remove a LAND scene published to a dcl-one-style content server (signed request)"
    )]
    Unpublish {
        #[arg(long, value_name = "X,Y")]
        parcel: String,
        #[arg(short = 't', long)]
        target: Option<String>,
        #[arg(long)]
        target_content: Option<String>,
        #[arg(long)]
        sign_key: Option<PathBuf>,
    },
    #[command(
        alias = "pack-smart-wearable",
        about = "Build and zip a smart wearable for upload to the builder"
    )]
    Pack {
        #[arg(long, default_value = ".", help = "Smart-wearable folder to pack")]
        dir: PathBuf,
        #[arg(long, help = "Zip the existing bin/ as-is instead of rebuilding")]
        skip_build: bool,
    },
    #[command(about = "Manage a world's settings and permissions on a worlds content server")]
    World {
        #[command(subcommand)]
        command: WorldCommand,
    },
    /// Build the prebuilt SDK runtime chunks that ship in the vendored blob.
    ///
    /// Hidden because it is a step of `scripts/build-base-blob.py`, not a scene
    /// command: `--dir` must point at a throwaway scene whose `node_modules` is
    /// the full blob install tree (including `@dcl/asset-packs` and
    /// `@dcl/sdk-commands`, neither of which the blob itself ships).
    #[command(hide = true)]
    VendorChunks {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        #[arg(long)]
        out_core: PathBuf,
        #[arg(long)]
        out_smart: PathBuf,
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
// A clap argv enum is parsed once and lives on one stack frame; boxing the
// large `Set` payload would cost flatten-compatibility for zero real gain.
#[allow(clippy::large_enum_variant)]
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
        #[command(flatten)]
        signed: SignedWriteArgs,
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
        #[command(flatten)]
        signed: SignedWriteArgs,
    },
    #[command(about = "Revoke a permission on a world from an address (signed request)")]
    Revoke {
        name: String,
        permission: String,
        address: String,
        #[command(flatten)]
        signed: SignedWriteArgs,
    },
}

/// The 5-arg block every signed world-mutation subcommand repeats: which content server to
/// hit, how to sign (headless key vs. browser), and how to drive the local signing page.
#[derive(Args)]
struct SignedWriteArgs {
    #[arg(long)]
    target_content: Option<String>,
    #[arg(long)]
    sign_key: Option<PathBuf>,
    #[arg(short = 'b', long)]
    no_browser: bool,
    #[arg(long)]
    ci: bool,
    #[arg(short, long)]
    port: Option<u16>,
}

impl SignedWriteArgs {
    fn browser_options(&self) -> world::BrowserOptions {
        world::BrowserOptions {
            port: self.port,
            no_browser: self.no_browser,
            ci: self.ci,
        }
    }
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
    ux::set_verbose(verbose);
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
        Command::VendorChunks {
            dir,
            out_core,
            out_smart,
        } => dcl_one_sdk::prebuilt::build_chunks(&dir, &out_core, &out_smart).await,
        Command::GetContextFiles { dir, offline } => {
            let api = std::env::var("DCL_ONE_SDK_CONTEXT_API")
                .unwrap_or_else(|_| context_files::DEFAULT_API.to_string());
            context_files::get_context_files(&dir, &api, offline).await
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
                let mut steps = ux::Steps::new(4);
                // The session type-checks after every rebuild, so the loop
                // reports errors as they are introduced rather than only for the
                // tree as it stood at startup.
                let session = watch::WatchSession::create(project, &opts, true, &mut steps).await?;
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
            skip_type_check,
            skip_install,
            no_watch,
            no_browser,
            ci,
            data_layer,
            ignore_composite,
            offline_comms,
            mini_comms,
            multi_instance,
            no_client,
            mobile,
            no_asset_bundles,
            asset_bundles,
            mcp,
            mcp_port,
            explorer_params,
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
            if multi_instance {
                ux::note("--multi-instance has no effect (the join block always prints a 2nd-instance deep link)");
            }
            if no_client {
                ux::note("--no-client has no effect (dcl-one-sdk never launches a client)");
            }
            start::start(start::StartOptions {
                dir,
                port,
                skip_build,
                skip_type_check,
                no_watch,
                ignore_composite,
                offline_comms,
                mobile,
                ab_sidecar: !no_asset_bundles && !asset_bundles,
                local_ab: asset_bundles,
                mcp,
                mcp_port,
                explorer_params,
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
        Command::Unpublish {
            parcel,
            target,
            target_content,
            sign_key,
        } => {
            deploy::unpublish(&deploy::UnpublishOptions {
                parcel,
                target,
                target_content,
                sign_key,
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
                signed,
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
                world::run_action(
                    &name,
                    world::WorldAction::SettingsSet(world::SettingsUpdate {
                        title,
                        description,
                        content_rating,
                        spawn_coordinates,
                        skybox_time,
                        single_player,
                        show_in_places,
                        categories,
                        thumbnail,
                    }),
                    signed.target_content.as_deref(),
                    signed.sign_key.as_deref(),
                    signed.browser_options(),
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
                signed,
            } => {
                world::run_action(
                    &name,
                    world::WorldAction::Permission {
                        permission,
                        address,
                        revoke: false,
                    },
                    signed.target_content.as_deref(),
                    signed.sign_key.as_deref(),
                    signed.browser_options(),
                )
                .await
            }
            WorldPermissionsCommand::Revoke {
                name,
                permission,
                address,
                signed,
            } => {
                world::run_action(
                    &name,
                    world::WorldAction::Permission {
                        permission,
                        address,
                        revoke: true,
                    },
                    signed.target_content.as_deref(),
                    signed.sign_key.as_deref(),
                    signed.browser_options(),
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
