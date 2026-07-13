use super::net::{
    build_delete_payload, confirm_world_overwrite, jump_in_url, resolve_target, send_world_delete,
    upload_entity,
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
    std::env::var_os("DCL_PRIVATE_KEY").is_some() || opts.sign_key.is_some()
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

fn prod_build_options(dir: &Path) -> build::BuildOptions {
    build::BuildOptions {
        dir: dir.to_path_buf(),
        production: true,
        ignore_composite: false,
        custom_entry_point: false,
        skip_type_check: false,
    }
}

fn print_entity_summary(entity_id: &str, timestamp: i64, files: &[(String, String, Vec<u8>)]) {
    println!("entityId={entity_id}");
    println!("timestamp={timestamp}");
    for (f, h, b) in files {
        println!("{h} {f} {} bytes", b.len());
    }
}

pub async fn deploy(opts: &DeployOptions) -> Result<()> {
    let project = Project::load(&opts.dir)?;
    let metadata = build_metadata(&project)?;
    let pointers = extract_pointers(&metadata)?;
    let world = world_name(&metadata);

    if opts.dry_run {
        if !opts.skip_build {
            build::build(&prod_build_options(&opts.dir)).await?;
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
        if let Some(path) = &opts.entity_out {
            std::fs::write(path, &entity_bytes)
                .with_context(|| format!("writing entity to {}", path.display()))?;
            tracing::info!("entity bytes written to {}", path.display());
        }
        tracing::info!("dry run — not uploading");
        ux::note("dry run \u{2014} entity not uploaded");
        return Ok(());
    }

    let headless = has_headless_signer(opts);
    let target = resolve_target(opts, world.as_deref(), headless).await?;
    let needs_delete = match &world {
        Some(w) if !opts.multi_scene => {
            confirm_world_overwrite(&target, w, &pointers, opts).await?
        }
        _ => false,
    };

    if !opts.skip_build {
        build::build(&prod_build_options(&opts.dir)).await?;
    }
    let prepared = prepare(&project)?;

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
    if let Some(path) = &opts.entity_out {
        std::fs::write(path, &entity_bytes)
            .with_context(|| format!("writing entity to {}", path.display()))?;
        tracing::info!("entity bytes written to {}", path.display());
    }

    let wallet = load_wallet(opts)?;
    let address = wallet.address();
    let signature = wallet
        .sign_message(entity_id.as_bytes())
        .context("EIP-191 sign")?;

    if needs_delete {
        if let Some(w) = world {
            let payload = build_delete_payload(w);
            let chain = catalyrst_crypto::create_simple_auth_chain(&wallet, &payload)
                .context("EIP-191 sign of the scene-removal payload")?;
            send_world_delete(target, w, &chain).await?;
        }
    }

    let message = upload_entity(
        target,
        &entity_id,
        entity_bytes,
        &prepared.files,
        &address,
        &signature,
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
    let mut steps = ux::Steps::new(2);
    for (f, h, b) in &prepared.files {
        println!("{h} {f} {} bytes", b.len());
    }
    steps.done(format!(
        "Entity prepared \u{2014} {} files (id minted at signing time)",
        prepared.files.len()
    ));
    let base = base_parcel(metadata, &prepared.pointers);
    let dep = linker::LinkerDeploy {
        prepared,
        target_content: target,
        world: world.clone(),
        needs_delete,
        timestamp_override: opts.timestamp,
        entity_out: opts.entity_out.clone(),
        scene_title: scene_title(metadata),
        base_parcel: base.clone(),
        multi_scene: opts.multi_scene,
    };
    let lopts = linker::LinkerOptions {
        port: opts.port,
        open_browser: !opts.no_browser && !opts.ci,
        timeout: linker::linker_timeout(),
    };
    let message = linker::run(dep, lopts).await?;
    steps.done(message);
    ux::note(jump_in_url(world.as_deref(), &base));
    Ok(())
}
