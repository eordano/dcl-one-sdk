use super::{now_ms, DeployOptions, CATALYST_ROTATION};
use crate::ux::{self, TrySteps, UserError};
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::collections::HashSet;
use std::io::{IsTerminal, Write};
use std::time::Duration;

fn probe_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("building the http client")
}

fn upload_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(300))
        .build()
        .context("building the upload http client")
}

pub(super) async fn resolve_target(
    opts: &DeployOptions,
    world: Option<&str>,
    headless: bool,
) -> Result<String> {
    match (&opts.target, &opts.target_content) {
        (Some(_), Some(_)) => Err(UserError::new(
            "pass either --target or --target-content, not both",
            TrySteps::one("--target <catalyst-domain> resolves the content server via /about")
                .and("--target-content <url> uploads to that content server verbatim"),
        )
        .into()),
        (None, Some(tc)) => Ok(tc.trim_end_matches('/').to_string()),
        (Some(t), None) => catalyst_content_url(t).await,
        (None, None) => {
            if let Ok(t) = std::env::var("DCL_ONE_SDK_DEFAULT_TARGET") {
                return default_env_target(&t).await;
            }
            if let Some(w) = world {
                return Err(UserError::new(
                    format!("this scene deploys to the world \"{w}\", which needs an explicit server"),
                    TrySteps::one("pass --target-content <worlds-content-server-url>")
                        .and("the public worlds server is https://worlds-content-server.decentraland.org"),
                )
                .into());
            }
            if headless {
                return Err(UserError::new(
                    "no deploy target given for key-based signing",
                    TrySteps::one(
                        "pass --target <catalyst-domain> or --target-content <content-server-url>",
                    )
                    .and("or set DCL_ONE_SDK_DEFAULT_TARGET=<catalyst-or-content-url>")
                    .and("browser signing (no key) picks a healthy public catalyst automatically"),
                )
                .why("key-signed deploys never pick a server implicitly")
                .into());
            }
            rotation_content_url().await
        }
    }
}

pub fn sanitize_catalyst_url(t: &str) -> String {
    let t = t.trim();
    let with_scheme = if t.contains("://") {
        t.to_string()
    } else {
        format!("https://{t}")
    };
    with_scheme.trim_end_matches('/').to_string()
}

async fn fetch_about(client: &reqwest::Client, base: &str) -> Result<serde_json::Value> {
    let url = format!("{base}/about");
    let resp = client.get(&url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        bail!("GET {url} returned HTTP {}", status.as_u16());
    }
    resp.json::<serde_json::Value>()
        .await
        .with_context(|| format!("parsing {url} as JSON"))
}

fn about_content_url(about: &serde_json::Value, base: &str) -> Option<String> {
    about
        .get("content")
        .and_then(|c| c.get("publicUrl"))
        .and_then(|u| u.as_str())
        .map(|u| {
            let u = u.trim_end_matches('/');
            if u.contains("://") {
                u.to_string()
            } else {
                format!("{base}{u}")
            }
        })
}

async fn catalyst_content_url(t: &str) -> Result<String> {
    let base = sanitize_catalyst_url(t);
    let client = probe_client()?;
    let about = fetch_about(&client, &base).await.map_err(|e| {
        anyhow::Error::from(
            UserError::new(
                format!("could not resolve the catalyst {base}"),
                TrySteps::one("check the domain and that the catalyst is up (GET <domain>/about)")
                    .and("for a raw content server, use --target-content <url> instead"),
            )
            .caused_by(std::io::Error::other(format!("{e:#}"))),
        )
    })?;
    about_content_url(&about, &base).ok_or_else(|| {
        UserError::new(
            format!("the catalyst {base} did not report a content server"),
            TrySteps::one("check <domain>/about returns content.publicUrl")
                .and("for a raw content server, use --target-content <url> instead"),
        )
        .into()
    })
}

async fn default_env_target(t: &str) -> Result<String> {
    let base = sanitize_catalyst_url(t);
    let client = probe_client()?;
    if let Ok(about) = fetch_about(&client, &base).await {
        if let Some(content) = about_content_url(&about, &base) {
            ux::note(format!(
                "using DCL_ONE_SDK_DEFAULT_TARGET catalyst {base} (content: {content})"
            ));
            return Ok(content);
        }
    }
    ux::note(format!(
        "using DCL_ONE_SDK_DEFAULT_TARGET as a content server: {base}"
    ));
    Ok(base)
}

async fn rotation_content_url() -> Result<String> {
    let client = probe_client()?;
    for base in CATALYST_ROTATION {
        match fetch_about(&client, base).await {
            Ok(about) => {
                let healthy = about
                    .get("healthy")
                    .and_then(|h| h.as_bool())
                    .unwrap_or(false);
                if !healthy {
                    continue;
                }
                if let Some(content) = about_content_url(&about, base) {
                    ux::note(format!("deploying via the public catalyst {base}"));
                    return Ok(content);
                }
            }
            Err(_) => continue,
        }
    }
    Err(UserError::new(
        "no public catalyst answered healthy",
        TrySteps::one("check your network connection")
            .and("or pass --target <catalyst-domain> / --target-content <url> explicitly"),
    )
    .into())
}

pub struct WorldScene {
    pub title: String,
    pub parcels: Vec<String>,
}

async fn fetch_world_scenes(target: &str, world: &str) -> Result<Vec<WorldScene>> {
    let client = probe_client()?;
    let url = format!("{target}/world/{}/scenes", encode_segment(world));
    let resp = client.get(&url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        bail!("GET {url} returned HTTP {}", status.as_u16());
    }
    let body: serde_json::Value = resp.json().await.context("parsing the world scenes list")?;
    let scenes = body
        .get("scenes")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(scenes
        .iter()
        .map(|s| WorldScene {
            title: s
                .get("entity")
                .and_then(|e| e.get("metadata"))
                .and_then(|m| m.get("display"))
                .and_then(|d| d.get("title"))
                .and_then(|t| t.as_str())
                .unwrap_or("Untitled")
                .to_string(),
            parcels: s
                .get("parcels")
                .and_then(|p| p.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect())
}

pub fn scenes_on_other_parcels<'a>(
    existing: &'a [WorldScene],
    deploying: &[String],
) -> Vec<&'a WorldScene> {
    let set: HashSet<&str> = deploying.iter().map(String::as_str).collect();
    existing
        .iter()
        .filter(|s| s.parcels.iter().all(|p| !set.contains(p.as_str())))
        .collect()
}

pub(super) async fn confirm_world_overwrite(
    target: &str,
    world: &str,
    deploying: &[String],
    opts: &DeployOptions,
) -> Result<bool> {
    let existing = match fetch_world_scenes(target, world).await {
        Ok(scenes) => scenes,
        Err(e) => {
            tracing::warn!("could not check existing scenes in {world}: {e:#}");
            return Ok(false);
        }
    };
    let others = scenes_on_other_parcels(&existing, deploying);
    if others.is_empty() {
        return Ok(false);
    }
    tracing::warn!(
        "World \"{world}\" has {} other scene(s) that will be removed:",
        others.len()
    );
    for s in &others {
        ux::note(format!(
            "  - \"{}\" at parcels {}",
            s.title,
            s.parcels.join(", ")
        ));
    }
    tracing::warn!(
        "Deploying without --multi-scene will DELETE all existing scenes in the world first."
    );
    if opts.yes {
        return Ok(true);
    }
    if opts.ci || !std::io::stdin().is_terminal() {
        return Err(UserError::new(
            format!(
                "this deploy would delete {} existing scene(s) in {world}",
                others.len()
            ),
            TrySteps::one("pass --multi-scene to deploy alongside them (no deletion)")
                .and("or pass --yes to confirm the deletion non-interactively"),
        )
        .into());
    }
    if prompt_continue()? {
        Ok(true)
    } else {
        Err(UserError::new(
            "deployment cancelled",
            TrySteps::one("pass --multi-scene to deploy alongside the existing scenes"),
        )
        .into())
    }
}

fn prompt_continue() -> Result<bool> {
    print!("Continue? (y/N) ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .context("reading the confirmation answer")?;
    let a = line.trim().to_ascii_lowercase();
    Ok(a == "y" || a == "yes")
}

pub fn build_delete_payload(world: &str) -> String {
    format!(
        "delete:/entities/{}:{}:{{}}",
        encode_segment(world),
        now_ms()
    )
    .to_lowercase()
}

pub fn simple_auth_chain(address: &str, payload: &str, signature: &str) -> serde_json::Value {
    json!([
        { "type": "SIGNER", "payload": address, "signature": "" },
        { "type": "ECDSA_SIGNED_ENTITY", "payload": payload, "signature": signature },
    ])
}

pub fn encode_segment(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub async fn send_world_delete(target: &str, world: &str, chain: &serde_json::Value) -> Result<()> {
    let links = chain.as_array().cloned().unwrap_or_default();
    let payload = links
        .last()
        .and_then(|l| l.get("payload"))
        .and_then(|p| p.as_str())
        .unwrap_or_default()
        .to_string();
    let parts: Vec<&str> = payload.split(':').collect();
    let timestamp = parts.get(2).copied().unwrap_or_default().to_string();
    let metadata = parts.get(3).copied().unwrap_or("{}").to_string();
    let url = format!("{target}/entities/{}", encode_segment(world));
    let mut req = upload_client()?
        .delete(&url)
        .header("x-identity-timestamp", timestamp)
        .header("x-identity-metadata", metadata);
    for (i, link) in links.iter().enumerate() {
        req = req.header(format!("x-identity-auth-chain-{i}"), link.to_string());
    }
    let resp = match req.send().await {
        Ok(resp) => resp,
        Err(e) => return Err(unreachable_server(&url, e)),
    };
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if status.is_success() {
        ux::note(format!(
            "removed the existing scenes in {world} (HTTP {})",
            status.as_u16()
        ));
        Ok(())
    } else {
        let mut u = UserError::new(
            format!(
                "the content server refused to delete the existing scenes in {world} (HTTP {})",
                status.as_u16()
            ),
            TrySteps::one(
                "use --multi-scene to deploy alongside existing scenes without deleting them",
            )
            .and("check the signing wallet has permission on the world"),
        );
        let body = body.trim();
        if !body.is_empty() {
            u = u.why(body);
        }
        Err(u.into())
    }
}

pub async fn upload_entity(
    target: &str,
    entity_id: &str,
    entity_bytes: Vec<u8>,
    files: &[(String, String, Vec<u8>)],
    address: &str,
    signature: &str,
) -> Result<String> {
    let auth_chain = simple_auth_chain(address, entity_id, signature);

    let mut form = reqwest::multipart::Form::new()
        .text("entityId", entity_id.to_string())
        .text("authChain", serde_json::to_string(&auth_chain)?)
        .text("authChain[0][type]", "SIGNER")
        .text("authChain[0][payload]", address.to_string())
        .text("authChain[0][signature]", "")
        .text("authChain[1][type]", "ECDSA_SIGNED_ENTITY")
        .text("authChain[1][payload]", entity_id.to_string())
        .text("authChain[1][signature]", signature.to_string());

    form = form.part(
        entity_id.to_string(),
        reqwest::multipart::Part::bytes(entity_bytes)
            .file_name(entity_id.to_string())
            .mime_str("application/json")?,
    );
    for (_rel, hash, bytes) in files {
        form = form.part(
            hash.clone(),
            reqwest::multipart::Part::bytes(bytes.clone()).file_name(hash.clone()),
        );
    }

    let url = format!("{}/entities", target.trim_end_matches('/'));
    tracing::info!("uploading to {url} as {address} (entity {entity_id})");
    ux::note(format!("uploading to {url} as {address}"));
    let resp = match upload_client()?.post(&url).multipart(form).send().await {
        Ok(resp) => resp,
        Err(e) => return Err(unreachable_server(&url, e)),
    };
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if status.is_success() {
        tracing::info!(
            "deployed \u{2713} (HTTP {}) — server: {body}",
            status.as_u16()
        );
        Ok(format!("Deployed {entity_id} (HTTP {})", status.as_u16()))
    } else {
        let pointers: Vec<String> = Vec::new();
        Err(rejected(status.as_u16(), &body, &pointers))
    }
}

pub fn jump_in_url(world: Option<&str>, base: &str) -> String {
    match world {
        Some(w) => format!("jump in: https://decentraland.org/play/?realm={w}"),
        None => format!("jump in: https://play.decentraland.org/?NETWORK=mainnet&position={base}"),
    }
}

fn unreachable_server(url: &str, e: reqwest::Error) -> anyhow::Error {
    let cause = if e.is_timeout() {
        "timed out"
    } else {
        classify_io(&e)
    };
    UserError::new(
        "could not reach the content server",
        TrySteps::one("check the server is running and the URL is right").and(
            "targets: --target <catalyst-domain>, --target-content <content-server-url> (e.g. a local worlds server on http://127.0.0.1:5142)",
        ),
    )
    .why(format!("{cause}: {url}"))
    .caused_by(e)
    .into()
}

fn classify_io(e: &(dyn std::error::Error + 'static)) -> &'static str {
    let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(s) = cur {
        if let Some(io) = s.downcast_ref::<std::io::Error>() {
            return match io.kind() {
                std::io::ErrorKind::ConnectionRefused => "connection refused",
                std::io::ErrorKind::TimedOut => "timed out",
                _ => "connection failed",
            };
        }
        cur = s.source();
    }
    "no response"
}

fn rejected(code: u16, body: &str, pointers: &[String]) -> anyhow::Error {
    let steps = if code == 401 || code == 403 {
        let what = if pointers.is_empty() {
            "the deployed pointers".to_string()
        } else {
            pointers.join(", ")
        };
        TrySteps::one(format!(
            "check the signing wallet owns or has permission on {what}"
        ))
        .and("re-run with --verbose for the full response")
    } else {
        TrySteps::one("read the server message above")
            .and("re-run with --verbose for the full response")
    };
    let mut u = UserError::new(
        format!("the content server rejected this deployment (HTTP {code})"),
        steps,
    );
    let body = body.trim();
    if !body.is_empty() {
        u = u.why(body);
    }
    u.into()
}
