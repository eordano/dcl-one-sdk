use super::net::{
    confirm_world_overwrite, delete_world_scenes, jump_in_url, non_upstream_note, resolve_target,
};
use super::{
    base_parcel, build_entity, build_metadata, extract_pointers, now_ms, prepare, scene_title,
    world_name, DeployOptions, Prepared,
};
use crate::build;
use crate::jsjson::JsValue;
use crate::linker;
use crate::scene::Project;
use crate::ux::{self, TrySteps, UserError};
use anyhow::{Context, Result};
use catalyrst_crypto::Wallet;
use std::path::Path;

fn has_headless_signer(opts: &DeployOptions) -> bool {
    std::env::var_os("DCL_PRIVATE_KEY").is_some()
        || opts.sign_key.is_some()
        || opts.identity.is_some()
}

pub fn load_signer(sign_key: Option<&Path>) -> Result<Option<Wallet>> {
    if let Some(path) = sign_key {
        let raw = std::fs::read_to_string(path).map_err(|e| {
            anyhow::Error::from(
                UserError::new(
                    format!("could not read the key file {}", path.display()),
                    TrySteps::one("check the --sign-key path"),
                )
                .caused_by(e),
            )
        })?;
        return Wallet::from_hex(&raw).map(Some).map_err(|e| {
            anyhow::Error::from(
                UserError::new(
                    format!("the key file {} is not a valid private key", path.display()),
                    TrySteps::one("expect 64 hex chars, 0x prefix optional"),
                )
                .caused_by(e),
            )
        });
    }
    if let Ok(pk) = std::env::var("DCL_PRIVATE_KEY") {
        let wallet = Wallet::from_hex(&pk).map_err(|e| {
            anyhow::Error::from(
                UserError::new(
                    "DCL_PRIVATE_KEY is not a valid private key",
                    TrySteps::one("expect 64 hex chars, 0x prefix optional")
                        .and("or pass --sign-key <path> (the flag wins over the env var)"),
                )
                .caused_by(e),
            )
        })?;
        ux::note_stderr(format!(
            "signing with DCL_PRIVATE_KEY from the environment (address {})",
            wallet.address()
        ));
        return Ok(Some(wallet));
    }
    Ok(None)
}

fn load_wallet(opts: &DeployOptions) -> Result<Wallet> {
    match load_signer(opts.sign_key.as_deref())? {
        Some(signer) => Ok(signer),
        None => Err(UserError::new(
            "no wallet available to sign the deployment",
            TrySteps::one("set DCL_PRIVATE_KEY=<hex> (CI / disposable operator key)")
                .and("or pass --sign-key <path-to-key-file>")
                .and("or drop both to sign with a browser wallet on the printed URL"),
        )
        .into()),
    }
}

fn prod_build_options(dir: &Path, quiet: bool) -> build::BuildOptions {
    build::BuildOptions {
        dir: dir.to_path_buf(),
        production: true,
        ignore_composite: false,
        custom_entry_point: false,
        skip_type_check: false,
        out_root: Some(dir.join(build::RELEASE_OUT)),
        quiet,
    }
}

/// CIDs are self-verifying noise to a human: eight leading and six trailing
/// characters are plenty to eyeball two rows apart, and the full hash still
/// rides the upload. Sizes get a traffic-light tint so the heavy files pop.
fn short_hash(h: &str) -> String {
    match h.len() > 15 {
        true => format!("{}\u{2026}{}", &h[..8], &h[h.len() - 6..]),
        false => h.to_string(),
    }
}

fn size_cell(len: usize) -> (&'static str, String) {
    let sgr = if len < 4 * 1024 {
        "32"
    } else if len < 64 * 1024 {
        "33"
    } else {
        "31"
    };
    let human = if len < 1024 {
        format!("{len} B")
    } else if len < 1024 * 1024 {
        format!("{:.1} KB", len as f64 / 1024.0)
    } else {
        format!("{:.2} MB", len as f64 / (1024.0 * 1024.0))
    };
    (sgr, human)
}

fn print_file_listing(files: &[(String, String, Vec<u8>)]) {
    let color = ux::stdout_color();
    let name_w = files
        .iter()
        .map(|(f, ..)| f.chars().count())
        .max()
        .unwrap_or(0);
    for (f, h, b) in files {
        let (sgr, human) = size_cell(b.len());
        println!(
            "{} {f:name_w$} {}",
            ux::tint(color, "2", &short_hash(h)),
            ux::tint(color, sgr, &format!("{human:>9}")),
        );
    }
}

fn print_entity_summary(entity_id: &str, timestamp: i64, files: &[(String, String, Vec<u8>)]) {
    println!("entityId={entity_id}");
    println!("timestamp={timestamp}");
    print_file_listing(files);
}

fn write_entity_out(opts: &DeployOptions, entity_bytes: &[u8]) -> Result<()> {
    if let Some(path) = &opts.entity_out {
        std::fs::write(path, entity_bytes)
            .with_context(|| format!("writing entity to {}", path.display()))?;
        tracing::info!("entity bytes written to {}", path.display());
    }
    Ok(())
}

pub async fn deploy(opts: &DeployOptions) -> Result<()> {
    let project = Project::load(&opts.dir)?;
    super::sticky_default_target(&project.root);
    let metadata = build_metadata(&project)?;
    let pointers = extract_pointers(&metadata)?;
    let world = world_name(&metadata);

    if opts.dry_run {
        if !opts.skip_build {
            build::build(&prod_build_options(&opts.dir, opts.quiet)).await?;
        }
        let prepared = prepare(&project)?;
        let timestamp = opts.timestamp.unwrap_or_else(now_ms);
        let (entity_id, entity_bytes) = build_entity(&prepared, timestamp)?;
        let mut steps = ux::Steps::new(1);
        print_entity_summary(&entity_id, timestamp, &prepared.files);
        steps.done(format!(
            "Entity packed \u{2014} {} files ({entity_id})",
            prepared.files.len()
        ));
        write_entity_out(opts, &entity_bytes)?;
        tracing::info!("dry run — not uploading");
        ux::note("dry run \u{2014} entity not uploaded");
        return Ok(());
    }

    let headless = has_headless_signer(opts);
    let target = resolve_target(opts, world.as_deref(), headless).await?;
    if world.is_none() && !opts.quiet {
        if let Some(note) = non_upstream_note(&target) {
            ux::note(note);
        }
    }
    // World deploys are additive by default: a scene replaces only what sits
    // on its own parcels and leaves the world's other scenes alone — the
    // preview's behaviour too, and the safe default. Additive also makes the
    // deploy ask the content server nothing before the upload; behind a
    // Cloudflare-fronted worlds server a pre-flight GET flags the IP and the
    // upload that follows is challenged, where a lone upload passes. The
    // destructive "replace every scene in the world" path is opt-in
    // (`multi_scene` is false only when the caller asked to replace), and it
    // makes the overwrite pre-check — so it works against a self-hosted
    // server but not behind a challenging edge.
    let needs_delete = match &world {
        Some(w) if !opts.multi_scene => {
            confirm_world_overwrite(&target, w, &pointers, opts).await?
        }
        _ => false,
    };

    if !opts.skip_build {
        build::build(&prod_build_options(&opts.dir, opts.quiet)).await?;
    }
    let prepared = {
        let project = project.clone();
        tokio::task::spawn_blocking(move || prepare(&project))
            .await
            .context("preparing the payload")??
    };

    if headless {
        deploy_headless(opts, prepared, &target, world.as_deref(), needs_delete).await
    } else {
        deploy_via_linker(opts, prepared, &metadata, target, world, needs_delete).await
    }
}

async fn deploy_headless(
    opts: &DeployOptions,
    prepared: Prepared,
    target: &str,
    world: Option<&str>,
    needs_delete: bool,
) -> Result<()> {
    let timestamp = opts.timestamp.unwrap_or_else(now_ms);
    let (entity_id, entity_bytes) = build_entity(&prepared, timestamp)?;
    let mut steps = ux::Steps::new(2);
    print_entity_summary(&entity_id, timestamp, &prepared.files);
    steps.done(format!(
        "Entity packed \u{2014} {} files ({entity_id})",
        prepared.files.len()
    ));
    write_entity_out(opts, &entity_bytes)?;

    // A delegated identity signs with its ephemeral key and a three-link
    // chain; otherwise the wallet (DCL_PRIVATE_KEY / --sign-key) signs
    // directly. Either way the ephemeral or the wallet, never both.
    let (address, auth_chain, signer_wallet) = match &opts.identity {
        Some(id) => {
            if id.expired(now_ms()) {
                return Err(UserError::new(
                    "the delegated deploy key has expired",
                    TrySteps::one("reconnect the account to mint a fresh one"),
                )
                .into());
            }
            let ephemeral = Wallet::from_hex(&id.ephemeral_key)
                .map_err(|e| anyhow::anyhow!("the delegated key is unusable: {e}"))?;
            let entity_sig = ephemeral
                .sign_message(entity_id.as_bytes())
                .context("EIP-191 sign (ephemeral)")?;
            let chain = crate::deploy::net::ephemeral_auth_chain(
                &id.signer,
                &id.delegation_payload,
                &id.delegation_signature,
                &entity_id,
                &entity_sig,
            );
            (id.signer.clone(), chain, None)
        }
        None => {
            let wallet = load_wallet(opts)?;
            let address = wallet.address();
            let signature = wallet
                .sign_message(entity_id.as_bytes())
                .context("EIP-191 sign")?;
            let chain = crate::deploy::net::simple_auth_chain(&address, &entity_id, &signature);
            (address, chain, Some(wallet))
        }
    };

    // No advisory permission pre-check before the upload: it is exactly the
    // pre-flight request whose bot score gets the upload challenged, and the
    // content server enforces permissions on the upload itself, returning a
    // clear refusal if the wallet may not publish here.
    if needs_delete {
        if let Some(w) = world {
            // The delete request is signed the same way the upload is: a
            // delegated identity cannot borrow the wallet to sign a delete.
            match &signer_wallet {
                Some(wallet) => delete_world_scenes(target, w, wallet).await?,
                None => {
                    return Err(UserError::new(
                        "a single-scene overwrite needs the wallet, not a delegated key",
                        TrySteps::one(
                            "drop --replace-world-scenes to add beside the world's other scenes",
                        )
                        .and("or deploy from a terminal with the wallet to replace them"),
                    )
                    .into());
                }
            }
        }
    }

    let message = crate::deploy::net::upload_entity_with_chain(
        target,
        &entity_id,
        entity_bytes,
        &prepared.files,
        &address,
        auth_chain,
    )
    .await?;
    steps.done(message);
    ux::note(jump_in_url(
        world,
        &base_parcel(&prepared.metadata, &prepared.pointers),
    ));
    Ok(())
}

async fn deploy_via_linker(
    opts: &DeployOptions,
    prepared: Prepared,
    metadata: &JsValue,
    target: String,
    world: Option<String>,
    needs_delete: bool,
) -> Result<()> {
    let mut steps = match opts.quiet {
        true => ux::Steps::silent(),
        false => ux::Steps::new(2),
    };
    if !opts.quiet {
        print_file_listing(&prepared.files);
    }
    steps.done(format!(
        "Entity prepared \u{2014} {} files (id minted at signing time)",
        prepared.files.len()
    ));
    let base = base_parcel(metadata, &prepared.pointers);
    let dep = linker::LinkerDeploy {
        dir: opts.dir.clone(),
        prepared,
        target_content: target,
        world: world.clone(),
        needs_delete,
        timestamp_override: opts.timestamp,
        entity_out: opts.entity_out.clone(),
        scene_title: scene_title(metadata),
        base_parcel: base.clone(),
        multi_scene: opts.multi_scene,
        // No advisory permission pre-check on the browser path either: the
        // pre-flight GET is what gets the upload challenged behind a
        // Cloudflare-fronted worlds server, and the server refuses the
        // upload itself when the wallet may not publish.
        check_permissions: false,
    };
    let lopts = linker::LinkerOptions {
        port: opts.port,
        open_browser: !opts.no_browser && !opts.ci,
        timeout: linker::linker_timeout(),
        host: opts.host_signer.clone(),
    };
    let message = linker::run(dep, lopts).await?;
    steps.done(message);
    if !opts.quiet {
        ux::note(jump_in_url(world.as_deref(), &base));
    }
    Ok(())
}
