use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use dcl_one_sdk::{
    build, context_files, deploy, init, joinblock, pack, scene, start, ux, watch, workspace, world,
};
use std::path::PathBuf;

/// Lowest MCP port the Explorer will actually serve on: below this it ignores
/// the deep link's `mcp-port` and falls back to its own default, which would
/// leave the scene-log poller waiting on a port nobody opened. Rejecting the
/// value here keeps the two ends from disagreeing silently.
const MIN_MCP_PORT: u16 = 1024;

/// The port both ends use when `--mcp-port` is absent.
fn resolved_mcp_port(mcp_port: Option<u16>) -> u16 {
    mcp_port.unwrap_or(joinblock::DEFAULT_EXPLORER_MCP_PORT)
}

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
            help = "Do not run the abgen asset-bundle sidecar, and stop forwarding local-ab=true with it. The sidecar is on by default and needs no install \u{2014} every dcl-one-sdk binary embeds abgen (ABGEN_BIN runs a different one). Upstream sdk-commands has no sidecar, so this is how to get its unoptimized preview"
        )]
        no_asset_bundles: bool,
        #[arg(
            long = "asset-bundles",
            conflicts_with = "no_asset_bundles",
            help = "Accepted for upstream CLI parity and does nothing: forwarding local-ab=true is now the default"
        )]
        asset_bundles: bool,
        #[arg(
            long,
            help = "Do not enable the Explorer's MCP server, and stop reading the running scene's errors out of it. MCP is on by default: the deep link carries mcp=true and mcp-port, and a scene that throws prints here instead of only in the client's log"
        )]
        no_mcp: bool,
        #[arg(
            long,
            help = "Let a machine other than this one press Deploy on the preview's /deploy page. Off by default: publishing signs with this machine's wallet, and the preview port is unauthenticated"
        )]
        allow_remote_deploy: bool,
        #[arg(
            long,
            conflicts_with = "no_mcp",
            help = "Accepted for parity with earlier releases and does nothing: enabling the Explorer's MCP server is now the default"
        )]
        mcp: bool,
        #[arg(
            long = "error-source-lines-context",
            value_name = "N",
            help = "Extra source lines either side of a scene error's line (default 0 \u{2014} just the line itself); --error-source-lines-before/-after override one side"
        )]
        error_lines_context: Option<u32>,
        #[arg(
            long = "error-source-lines-before",
            value_name = "N",
            help = "Source lines to show BEFORE a scene error's line"
        )]
        error_source_lines_before: Option<u32>,
        #[arg(
            long = "error-source-lines-after",
            value_name = "N",
            help = "Source lines to show AFTER a scene error's line"
        )]
        error_source_lines_after: Option<u32>,
        #[arg(
            long = "mcp-port",
            value_name = "PORT",
            conflicts_with = "no_mcp",
            value_parser = clap::value_parser!(u16).range(i64::from(MIN_MCP_PORT)..=i64::from(u16::MAX)),
            help = "Port for the Explorer's MCP server (1024-65535), forwarded into the desktop deep link. Defaults to the Explorer's own default port, so both ends agree without being told"
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
        #[arg(
            long,
            help = "Do not attach the authoritative-server isolate a scene.json authoritativeMultiplayer flag would auto-start"
        )]
        no_host: bool,
    },
    #[command(
        about = "Run the scene's authoritative-server isolate against a running preview's room"
    )]
    Host {
        #[arg(long, default_value = ".", help = "Project folder to host")]
        dir: PathBuf,
        #[arg(
            long,
            default_value = "http://127.0.0.1:8000",
            help = "The preview server whose mini-comms room this host joins"
        )]
        preview: String,
        #[arg(long, default_value = "room-1", help = "Room id on the preview")]
        room: String,
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
        #[arg(
            long,
            help = "Replace ALL scenes in the world with this one (default: additive — deploy alongside, keep the others). Destructive, and needs a pre-flight check that a Cloudflare-fronted worlds server challenges"
        )]
        replace_world_scenes: bool,
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
    /// Generate main.crdt from a scene's composites into an arbitrary file.
    /// Hidden: it exists so the native generator's bytes can be diffed against a
    /// node data-layer dump without running a build.
    #[command(hide = true)]
    CrdtGen {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Build the prebuilt SDK runtime chunks that ship in the vendored blob.
    /// Hidden: a step of `scripts/build-base-blob.py`, so `--dir` must be a
    /// throwaway scene whose `node_modules` is the full blob install tree
    /// (including `@dcl/asset-packs` and `@dcl/sdk-commands`, which the blob
    /// itself does not ship).
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
    // Success leaves the same abrupt way the error path always has. Falling
    // off the end instead would drop the tokio runtime, and that drop waits
    // for whatever blocking walk or stubborn child is still out there — which
    // reads as a ctrl-c that does not stop. A CLI has nothing to flush.
    std::process::exit(0);
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
        Command::CrdtGen { dir, out } => {
            let generated = dcl_one_sdk::crdt_gen::generate(&dir)?
                .ok_or_else(|| anyhow::anyhow!("no .composite files under {}", dir.display()))?;
            std::fs::write(&out, &generated.bytes)?;
            println!(
                "{} bytes from {} composite(s) -> {}",
                generated.bytes.len(),
                generated.composites,
                out.display()
            );
            Ok(())
        }
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
                out_root: None,
                quiet: false,
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
            multi_instance,
            no_client,
            mobile,
            no_asset_bundles,
            asset_bundles: _,
            no_mcp,
            allow_remote_deploy,
            mcp: _,
            mcp_port,
            error_lines_context,
            error_source_lines_before,
            error_source_lines_after,
            explorer_params,
            tunnel,
            tunnel_token,
            tunnel_token_file,
            no_host,
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
                ab_sidecar: !no_asset_bundles,
                local_ab: !no_asset_bundles,
                mcp: !no_mcp,
                allow_remote_deploy,
                mcp_port: resolved_mcp_port(mcp_port),
                source_context: start::SourceContext::resolve(
                    error_lines_context,
                    error_source_lines_before,
                    error_source_lines_after,
                ),
                explorer_params,
                data_layer,
                tunnel,
                tunnel_token,
                no_host,
            })
            .await
        }
        Command::Host { dir, preview, room } => {
            dcl_one_sdk::host::host(&dcl_one_sdk::host::HostOptions { dir, preview, room }).await
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
            replace_world_scenes,
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
                // Additive is the default; the flag opts into replacing all.
                multi_scene: !replace_world_scenes,
                yes,
                no_browser,
                ci,
                port,
                quiet: false,
                host_signer: None,
                identity: None,
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
            match build::type_check(session.project(), build::Reloaded::Yes).await {
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    fn parse(args: &[&str]) -> Command {
        let mut argv = vec!["dcl-one-sdk", "start"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv)
            .unwrap_or_else(|e| panic!("expected `start {args:?}` to parse: {e}"))
            .command
    }

    /// (no_mcp, mcp, mcp_port) of a parsed `start`.
    fn mcp_flags(args: &[&str]) -> (bool, bool, Option<u16>) {
        match parse(args) {
            Command::Start {
                no_mcp,
                mcp,
                mcp_port,
                ..
            } => (no_mcp, mcp, mcp_port),
            _ => panic!("not a start command"),
        }
    }

    fn start_err(args: &[&str]) -> clap::Error {
        let mut argv = vec!["dcl-one-sdk", "start"];
        argv.extend_from_slice(args);
        match Cli::try_parse_from(argv) {
            Ok(_) => panic!("expected `start {args:?}` to be rejected"),
            Err(e) => e,
        }
    }

    #[test]
    fn mcp_is_on_by_default_and_agrees_with_the_explorers_own_port() {
        assert_eq!(mcp_flags(&[]), (false, false, None));
        assert_eq!(
            resolved_mcp_port(None),
            joinblock::DEFAULT_EXPLORER_MCP_PORT
        );
        assert_eq!(resolved_mcp_port(Some(9111)), 9111);
    }

    #[test]
    fn bare_mcp_is_accepted_and_changes_nothing() {
        assert_eq!(mcp_flags(&["--mcp"]), (false, true, None));
        let (no_mcp, _, mcp_port) = mcp_flags(&["--mcp"]);
        assert_eq!((no_mcp, resolved_mcp_port(mcp_port)), {
            let (d_no_mcp, _, d_port) = mcp_flags(&[]);
            (d_no_mcp, resolved_mcp_port(d_port))
        });
    }

    #[test]
    fn no_mcp_conflicts_with_mcp_and_with_mcp_port() {
        assert_eq!(mcp_flags(&["--no-mcp"]), (true, false, None));
        assert_eq!(
            start_err(&["--mcp", "--no-mcp"]).kind(),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            start_err(&["--no-mcp", "--mcp-port", "8123"]).kind(),
            ErrorKind::ArgumentConflict
        );
    }

    /// The Explorer clamps anything below 1024 to its own default and serves
    /// there, while the scene-log poller would keep polling the port we asked
    /// for: reject the value instead of letting the two ends disagree.
    #[test]
    fn mcp_port_rejects_values_the_explorer_would_not_serve() {
        for bad in ["0", "1", "1023", "70000", "abc"] {
            let err = start_err(&["--mcp-port", bad]);
            assert_eq!(
                err.kind(),
                ErrorKind::ValueValidation,
                "--mcp-port {bad} should be rejected"
            );
        }
        assert!(Cli::try_parse_from(["dcl-one-sdk", "start", "--mcp-port", "-1"]).is_err());
        let msg = start_err(&["--mcp-port", "0"]).to_string();
        assert!(
            msg.contains("--mcp-port") && msg.contains("1024") && msg.contains("65535"),
            "unhelpful error: {msg}"
        );
        assert_eq!(mcp_flags(&["--mcp-port", "1024"]).2, Some(1024));
        assert_eq!(mcp_flags(&["--mcp-port", "65535"]).2, Some(65535));
        assert_eq!(
            mcp_flags(&["--mcp-port", "8123"]).2,
            Some(joinblock::DEFAULT_EXPLORER_MCP_PORT)
        );
    }
}
