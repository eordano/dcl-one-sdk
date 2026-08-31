use super::{
    catalyst_rotation, configured_catalyst_rotation, now_ms, DeployOptions, UPSTREAM_CATALYST_HOSTS,
};
use crate::ux::{self, TrySteps, UserError};
use anyhow::{bail, Context, Result};
use catalyrst_crypto::Wallet;
use serde_json::json;
use std::collections::HashSet;
use std::io::{IsTerminal, Write};
use std::time::Duration;

/// The canonical public server for named worlds. A scene whose
/// `worldConfiguration.name` is set already names its destination, so this is
/// what a world deploy resolves to when no flag or env default says otherwise.
pub const WORLDS_CONTENT_SERVER: &str = "https://worlds-content-server.decentraland.org";

pub(crate) fn client(connect: Duration, total: Duration) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        // Honest identification: reqwest sends no User-Agent by default, and
        // an anonymous client scores as junk with every WAF between here and
        // a self-hosted realm. Saying who we are is not a bypass — an edge
        // that challenges still challenges — it just stops the requests
        // reading as nobody's.
        .user_agent(concat!("dcl-one-sdk/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(connect)
        .timeout(total)
        .build()
        .context("building the http client")
}

fn probe_client() -> Result<reqwest::Client> {
    client(Duration::from_secs(10), Duration::from_secs(10))
}

fn upload_client() -> Result<reqwest::Client> {
    client(Duration::from_secs(10), Duration::from_secs(300))
}

/// Upload the entity with Node's `fetch`. A Cloudflare-fronted worlds server
/// challenges reqwest and curl but NOT Node — the official Creator Hub and
/// sdk-commands are Node, so its request fingerprint is the one the edge is
/// built to accept. The SDK already needs Node to build, so it is here. The
/// bytes are staged to a temp dir and a tiny module POSTs them; `None` means
/// Node is absent (fall back to curl, then reqwest). Same signed entity,
/// different sender.
async fn node_upload(
    url: &str,
    entity_id: &str,
    entity_bytes: &[u8],
    files: &[(String, String, Vec<u8>)],
    auth_chain: &serde_json::Value,
) -> Option<Result<(u16, String)>> {
    let node = crate::build::find_node()?;
    let dir = std::env::temp_dir().join(format!("dcl-one-sdk-nodeup-{entity_id}"));
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&dir);
    };
    let stage = || -> std::io::Result<()> {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(entity_id), entity_bytes)?;
        for (_rel, hash, bytes) in files {
            std::fs::write(dir.join(hash), bytes)?;
        }
        let cfg = serde_json::json!({
            "url": url,
            "dir": dir.to_string_lossy(),
            "entityId": entity_id,
            "authChain": auth_chain,
            "files": files.iter().map(|(_, h, _)| h).collect::<Vec<_>>(),
        });
        std::fs::write(dir.join("cfg.json"), serde_json::to_vec(&cfg)?)?;
        std::fs::write(dir.join("up.mjs"), NODE_UPLOAD_MJS)?;
        Ok(())
    };
    if stage().is_err() {
        cleanup();
        return Some(Err(anyhow::anyhow!("could not stage the upload")));
    }
    let out = tokio::process::Command::new(&node)
        .arg(dir.join("up.mjs"))
        .arg(dir.join("cfg.json"))
        .output()
        .await;
    cleanup();
    match out {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let v: serde_json::Value = match serde_json::from_str(stdout.trim()) {
                Ok(v) => v,
                // No JSON on stdout means the fetch threw before a response —
                // a connection refused / DNS failure / timeout. Report it as
                // status 0, which the caller renders as "could not reach the
                // content server", the same as the reqwest transport error.
                Err(_) => return Some(Ok((0, String::new()))),
            };
            let code = v.get("status").and_then(|s| s.as_u64()).unwrap_or(0) as u16;
            let body = v
                .get("body")
                .and_then(|b| b.as_str())
                .unwrap_or("")
                .to_string();
            Some(Ok((code, body)))
        }
        Err(e) => Some(Err(anyhow::anyhow!("node could not run: {e}"))),
    }
}

/// The module `node_upload` runs. Node 18+ has global `fetch`/`FormData`/
/// `Blob`; it reads the staged bytes and POSTs the multipart, printing one
/// line of JSON with the status and body.
const NODE_UPLOAD_MJS: &str = r#"import { readFileSync } from 'node:fs';
import { join } from 'node:path';
const cfg = JSON.parse(readFileSync(process.argv[2], 'utf8'));
const blob = (name, type) => new Blob([readFileSync(join(cfg.dir, name))], type ? { type } : undefined);
const fd = new FormData();
fd.append('entityId', cfg.entityId);
fd.append('authChain', JSON.stringify(cfg.authChain));
cfg.authChain.forEach((l, i) => {
  fd.append(`authChain[${i}][type]`, l.type);
  fd.append(`authChain[${i}][payload]`, l.payload);
  fd.append(`authChain[${i}][signature]`, l.signature);
});
fd.append(cfg.entityId, blob(cfg.entityId, 'application/json'), cfg.entityId);
for (const hash of cfg.files) fd.append(hash, blob(hash, 'application/octet-stream'), hash);
try {
  const r = await fetch(cfg.url, { method: 'POST', body: fd });
  const body = await r.text();
  process.stdout.write(JSON.stringify({ status: r.status, body }));
} catch (e) {
  process.stderr.write(String(e && e.message ? e.message : e));
  process.exit(1);
}
"#;

/// Upload the entity with curl. A Cloudflare-fronted worlds server
/// challenges the reqwest upload — its request fingerprint (multipart
/// boundary, header order and casing) reads as automated where curl's does
/// not. Node is preferred (its fingerprint is the one the edge accepts);
/// curl is the fallback when Node is absent. `None` means curl is absent
/// too. The signed entity is identical — this changes who sends, not what.
async fn curl_upload(
    url: &str,
    entity_id: &str,
    entity_bytes: &[u8],
    files: &[(String, String, Vec<u8>)],
    auth_chain: &serde_json::Value,
) -> Option<Result<(u16, String)>> {
    let dir = std::env::temp_dir().join(format!("dcl-one-sdk-upload-{entity_id}"));
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&dir);
    };
    if std::fs::create_dir_all(&dir).is_err()
        || std::fs::write(dir.join(entity_id), entity_bytes).is_err()
    {
        return Some(Err(anyhow::anyhow!("could not stage the upload")));
    }
    let mut cmd = tokio::process::Command::new("curl");
    cmd.arg("-sS")
        .args(["-X", "POST"])
        .args(["-A", concat!("dcl-one-sdk/", env!("CARGO_PKG_VERSION"))])
        .args(["-w", "\n%{http_code}"])
        .arg("-F")
        .arg(format!("entityId={entity_id}"))
        .arg("-F")
        .arg(format!(
            "authChain={}",
            serde_json::to_string(auth_chain).ok()?
        ));
    for (i, link) in auth_chain.as_array().into_iter().flatten().enumerate() {
        let f = |k: &str| link.get(k).and_then(|v| v.as_str()).unwrap_or("");
        for k in ["type", "payload", "signature"] {
            cmd.arg("-F").arg(format!("authChain[{i}][{k}]={}", f(k)));
        }
    }
    cmd.arg("-F").arg(format!(
        "{entity_id}=@{};type=application/json;filename={entity_id}",
        dir.join(entity_id).display()
    ));
    for (_rel, hash, bytes) in files {
        if std::fs::write(dir.join(hash), bytes).is_err() {
            cleanup();
            return Some(Err(anyhow::anyhow!("could not stage a payload file")));
        }
        cmd.arg("-F").arg(format!(
            "{hash}=@{};type=application/octet-stream;filename={hash}",
            dir.join(hash).display()
        ));
    }
    cmd.arg(url);
    let out = cmd.output().await;
    cleanup();
    match out {
        Ok(o) => {
            let combined = String::from_utf8_lossy(&o.stdout);
            let (body, code) = match combined.rsplit_once('\n') {
                Some((b, c)) => (b.to_string(), c.trim().parse::<u16>().unwrap_or(0)),
                None => (combined.to_string(), 0),
            };
            Some(Ok((code, body)))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(Err(anyhow::anyhow!("curl could not run: {e}"))),
    }
}

/// How far the caller has already consented to a target being chosen for it.
/// Only consulted when nothing named one, which is the single path on which
/// this CLI can reach the public network without being asked to.
#[derive(Clone, Copy, Default)]
pub(super) struct TargetConsent {
    pub assume_yes: bool,
    pub non_interactive: bool,
    /// The chosen-for-you note stays off the terminal: the page-driven flow
    /// already names the destination on the page that consented to it.
    pub quiet: bool,
}

pub(super) async fn resolve_target(
    opts: &DeployOptions,
    world: Option<&str>,
    headless: bool,
) -> Result<String> {
    resolve_target_from(
        opts.target.as_deref(),
        opts.target_content.as_deref(),
        world,
        headless,
        TargetConsent {
            assume_yes: opts.yes,
            non_interactive: opts.ci,
            quiet: opts.quiet,
        },
    )
    .await
}

pub(super) async fn resolve_target_from(
    target: Option<&str>,
    target_content: Option<&str>,
    world: Option<&str>,
    headless: bool,
    consent: TargetConsent,
) -> Result<String> {
    match (target, target_content) {
        (Some(_), Some(_)) => Err(UserError::new(
            "pass either --target or --target-content, not both",
            TrySteps::one("--target <catalyst-domain> resolves the content server via /about")
                .and("--target-content <url> uploads to that content server verbatim"),
        )
        .into()),
        (None, Some(tc)) => Ok(tc.trim_end_matches('/').to_string()),
        (Some(t), None) => catalyst_content_url(t).await,
        (None, None) => {
            if let Some(t) = env_default_target() {
                return default_env_target(&t).await;
            }
            if let Some(w) = world {
                ux::note(format!(
                    "deploying the world \"{w}\" to the public worlds server {WORLDS_CONTENT_SERVER}"
                ));
                return Ok(WORLDS_CONTENT_SERVER.to_string());
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
            rotation_content_url(consent).await
        }
    }
}

pub fn non_upstream_note(target: &str) -> Option<String> {
    let host = host_of(target)?;
    let upstream = UPSTREAM_CATALYST_HOSTS
        .iter()
        .any(|r| host_of(r).is_some_and(|h| h.eq_ignore_ascii_case(&host)));
    if upstream {
        None
    } else {
        Some(format!(
            "publishing to {host}: this updates that network only, not Genesis City on decentraland.org"
        ))
    }
}

pub(crate) fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

pub(super) fn url_path(base: &str) -> String {
    let rest = base.split_once("://").map_or(base, |(_, r)| r);
    match rest.find('/') {
        Some(i) => rest[i..].to_string(),
        None => String::new(),
    }
}

/// A blank `DCL_ONE_SDK_DEFAULT_TARGET` means "unset", not "deploy to the empty
/// string": it sanitizes to a bare "https:", which no server answers. The
/// landing page reads it the same way when it prints the deploy command, so
/// both have to fall through to the world branch or the rotation together.
pub fn env_default_target() -> Option<String> {
    std::env::var("DCL_ONE_SDK_DEFAULT_TARGET")
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// The variable rides the environment, and a bare restart silently falls
/// back to the public servers -- a world deploy aimed at a self-hosted realm
/// walks into the upstream wall instead. So the value sticks per scene: a
/// run with the variable writes `.dcl-one/deploy-target`, a run without one
/// adopts what is written there (and says which file spoke, so an
/// intentional return to the public servers knows what to delete).
pub fn sticky_default_target(root: &std::path::Path) {
    let path = root.join(".dcl-one").join("deploy-target");
    if let Some(t) = env_default_target() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, format!("{t}\n"));
        return;
    }
    let Ok(saved) = std::fs::read_to_string(&path) else {
        return;
    };
    let saved = saved.trim();
    if saved.is_empty() {
        return;
    }
    std::env::set_var("DCL_ONE_SDK_DEFAULT_TARGET", saved);
    ux::note(format!(
        "deploy target {saved} remembered from {} (delete that file to use the public servers)",
        path.display()
    ));
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
    // Used verbatim as a content server — no `/about` probe. That probe was
    // one reqwest GET to the deploy host before the upload, and behind a
    // Cloudflare-fronted worlds server it flags the IP so the upload that
    // follows is challenged. A catalyst domain that needs `/about`
    // resolution goes through `--target` instead; the env default is for a
    // content-server URL.
    let base = sanitize_catalyst_url(t);
    ux::note(format!(
        "using DCL_ONE_SDK_DEFAULT_TARGET as a content server: {base}"
    ));
    Ok(base)
}

/// Nothing named a target, so one is about to be picked, and the built-in
/// rotation publishes to Genesis City: say so and get a yes before uploading.
fn consent_to_public_deploy(base: &str, consent: TargetConsent) -> Result<()> {
    let host = host_of(base).unwrap_or_else(|| base.to_string());
    if !consent.quiet {
        ux::note(format!(
            "no --target given \u{2014} publishing to the public Genesis City network via {host}"
        ));
    }
    if consent.assume_yes {
        return Ok(());
    }
    if consent.non_interactive || !std::io::stdin().is_terminal() {
        return Err(UserError::new(
            format!("this deploy would publish to the public Genesis City network via {base}"),
            TrySteps::one(
                "pass --target <catalyst-domain> or --target-content <url> to publish elsewhere",
            )
            .and("or set DCL_ONE_SDK_DEFAULT_TARGET=<catalyst-or-content-url>")
            .and("or pass --yes to confirm the public deploy non-interactively"),
        )
        .why("no target was given, so the target was chosen for you")
        .into());
    }
    if prompt_continue()? {
        Ok(())
    } else {
        Err(UserError::new(
            "deployment cancelled",
            TrySteps::one("pass --target <catalyst-domain> to publish somewhere else"),
        )
        .into())
    }
}

async fn rotation_content_url(consent: TargetConsent) -> Result<String> {
    let configured = configured_catalyst_rotation();
    let rotation = configured.clone().unwrap_or_else(catalyst_rotation);
    let client = probe_client()?;
    // Probed concurrently, taken in rotation-priority order: a dead network
    // costs one timeout, not one per host.
    let probes: Vec<_> = rotation
        .iter()
        .map(|base| {
            let client = client.clone();
            let base = base.clone();
            tokio::spawn(async move {
                let about = fetch_about(&client, &base).await.ok()?;
                if !about
                    .get("healthy")
                    .and_then(|h| h.as_bool())
                    .unwrap_or(false)
                {
                    return None;
                }
                about_content_url(&about, &base)
            })
        })
        .collect();
    for (base, probe) in rotation.iter().zip(probes) {
        if let Ok(Some(content)) = probe.await {
            if configured.is_some() {
                ux::note(format!(
                    "deploying via {base} from DCL_ONE_SDK_CATALYST_ROTATION"
                ));
            } else {
                consent_to_public_deploy(base, consent)?;
            }
            return Ok(content);
        }
    }
    Err(UserError::new(
        "no catalyst in the rotation answered healthy",
        TrySteps::one("check your network connection")
            .and("or pass --target <catalyst-domain> / --target-content <url> explicitly"),
    )
    .into())
}

pub struct WorldScene {
    pub title: String,
    pub parcels: Vec<String>,
    pub timestamp: Option<i64>,
    pub content_hashes: Vec<String>,
    /// Deployed bytes, when the server reports them (the row's `size` is a
    /// stringified integer on servers that do; absent elsewhere).
    pub size: Option<u64>,
}

pub(crate) fn entity_title(entity: &serde_json::Value) -> String {
    entity
        .get("metadata")
        .and_then(|m| m.get("display"))
        .and_then(|d| d.get("title"))
        .and_then(|t| t.as_str())
        .unwrap_or("Untitled")
        .to_string()
}

pub(crate) fn entity_content_hashes(entity: &serde_json::Value) -> Vec<String> {
    entity
        .get("content")
        .and_then(|c| c.as_array())
        .map(|content| {
            content
                .iter()
                .filter_map(|f| f.get("hash").and_then(|h| h.as_str()))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn parse_world_scenes(body: &serde_json::Value) -> Vec<WorldScene> {
    body.get("scenes")
        .and_then(|s| s.as_array())
        .map(|scenes| {
            scenes
                .iter()
                .map(|s| {
                    let entity = s.get("entity").cloned().unwrap_or_default();
                    WorldScene {
                        title: entity_title(&entity),
                        parcels: s
                            .get("parcels")
                            .and_then(|p| p.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        timestamp: entity.get("timestamp").and_then(|t| t.as_i64()),
                        content_hashes: entity_content_hashes(&entity),
                        size: s.get("size").and_then(|v| match v {
                            serde_json::Value::String(s) => s.parse().ok(),
                            other => other.as_u64(),
                        }),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
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
    Ok(parse_world_scenes(&body))
}

pub struct PermissionCheck {
    pub allowed: bool,
    pub denied_parcels: Vec<String>,
    /// The permissions document the decision was made from. Carried so a
    /// refusal can show who owns the world and who was granted what, instead
    /// of leaving the user to go and ask.
    pub doc: serde_json::Value,
}

/// What the permissions document alone says about an address. The scoped
/// list is a second request, so the decision is split where the protocol
/// splits it: this half needs one document, and only `NeedsParcels` pays for
/// the other one. Pure, because both the deploy and the /target page decide
/// with it, and only fixtures can prove they decide alike.
pub(crate) enum DocAnswer {
    Granted,
    NeedsParcels,
}

pub(crate) fn deployment_permission_in_doc(doc: &serde_json::Value, address: &str) -> DocAnswer {
    if doc
        .get("owner")
        .and_then(|o| o.as_str())
        .is_some_and(|o| o.eq_ignore_ascii_case(address))
    {
        return DocAnswer::Granted;
    }
    if let Some(dep) = doc.get("permissions").and_then(|p| p.get("deployment")) {
        if dep.get("type").and_then(|t| t.as_str()) == Some("unrestricted") {
            return DocAnswer::Granted;
        }
        let in_wallets = dep
            .get("wallets")
            .and_then(|w| w.as_array())
            .is_some_and(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .any(|w| w.eq_ignore_ascii_case(address))
            });
        if in_wallets {
            let world_wide = doc
                .get("summary")
                .and_then(|s| s.get(address.to_lowercase()))
                .and_then(|entries| entries.as_array())
                .and_then(|arr| {
                    arr.iter().find(|e| {
                        e.get("permission").and_then(|p| p.as_str()) == Some("deployment")
                    })
                })
                .map(|e| {
                    e.get("world_wide")
                        .and_then(|w| w.as_bool())
                        .unwrap_or(false)
                });
            if world_wide.unwrap_or(true) {
                return DocAnswer::Granted;
            }
        }
    }
    DocAnswer::NeedsParcels
}

/// The deploying pointers the scoped-grant list does NOT cover — the parcels
/// a deploy would be refused for.
pub(crate) fn denied_parcels_in(scoped: &serde_json::Value, deploying: &[String]) -> Vec<String> {
    let allowed: HashSet<&str> = scoped
        .get("parcels")
        .and_then(|p| p.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    deploying
        .iter()
        .filter(|p| !allowed.contains(p.as_str()))
        .cloned()
        .collect()
}

async fn check_world_deployment_permission(
    target: &str,
    world: &str,
    address: &str,
    deploying: &[String],
) -> Result<PermissionCheck> {
    let client = probe_client()?;
    let base = target.trim_end_matches('/');
    let url = format!("{base}/world/{}/permissions", encode_segment(world));
    let resp = client.get(&url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        bail!("GET {url} returned HTTP {}", status.as_u16());
    }
    let body: serde_json::Value = resp.json().await.context("parsing the world permissions")?;
    if matches!(
        deployment_permission_in_doc(&body, address),
        DocAnswer::Granted
    ) {
        return Ok(PermissionCheck {
            allowed: true,
            denied_parcels: Vec::new(),
            doc: body,
        });
    }
    let url = format!(
        "{base}/world/{}/permissions/deployment/address/{}/parcels",
        encode_segment(world),
        address.to_lowercase()
    );
    let resp = client.get(&url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        bail!("GET {url} returned HTTP {}", status.as_u16());
    }
    let parcel_body: serde_json::Value = resp
        .json()
        .await
        .context("parsing the parcel permissions")?;
    let denied_parcels = denied_parcels_in(&parcel_body, deploying);
    Ok(PermissionCheck {
        allowed: denied_parcels.is_empty(),
        denied_parcels,
        doc: body,
    })
}

pub async fn enforce_world_permission(
    target: &str,
    world: &str,
    address: &str,
    deploying: &[String],
) -> Result<()> {
    match check_world_deployment_permission(target, world, address, deploying).await {
        Ok(check) if check.allowed => {
            ux::note(format!(
                "deploy permission on \"{world}\" verified for {address}"
            ));
            Ok(())
        }
        Ok(check) => {
            let denied = if check.denied_parcels.is_empty() {
                String::new()
            } else {
                format!(" (parcels: {})", check.denied_parcels.join(", "))
            };
            let owner = check
                .doc
                .get("owner")
                .and_then(|o| o.as_str())
                .unwrap_or("(unknown)");
            Err(UserError::new(
                format!(
                    "wallet {address} has no permission to deploy to world \"{world}\"{denied}"
                ),
                TrySteps::one(format!(
                    "ask {owner} to grant it: dcl-one-sdk world permissions grant {world} deployment {address}"
                ))
                .and("or sign with a wallet listed below"),
            )
            .why(crate::world::render_permissions(world, &check.doc))
            .into())
        }
        Err(e) => {
            tracing::warn!("could not verify deployment permissions: {e:#}");
            Ok(())
        }
    }
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
        "Replacing the world: this DELETES all its other scenes first (--replace-world-scenes)."
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
            TrySteps::one(
                "drop --replace-world-scenes to deploy alongside them (additive, the default)",
            )
            .and("or pass --yes to confirm the deletion non-interactively"),
        )
        .into());
    }
    if prompt_continue()? {
        Ok(true)
    } else {
        Err(UserError::new(
            "deployment cancelled",
            TrySteps::one("drop --replace-world-scenes to deploy alongside the existing scenes"),
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

async fn world_delete_request(
    target: &str,
    world: &str,
    chain: &serde_json::Value,
) -> Result<(u16, String)> {
    let links = chain.as_array().cloned().unwrap_or_default();
    let payload = links
        .last()
        .and_then(|l| l.get("payload"))
        .and_then(|p| p.as_str())
        .unwrap_or_default()
        .to_string();
    let url = format!("{target}/entities/{}", encode_segment(world));
    let mut req = upload_client()?.delete(&url);
    for (name, value) in crate::world::headers_from_chain(&payload, chain) {
        req = req.header(name, value);
    }
    let resp = match req.send().await {
        Ok(resp) => resp,
        Err(e) => return Err(unreachable_server(&url, e)),
    };
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    Ok((status, body))
}

fn world_delete_refused(world: &str, status: u16, body: &str) -> anyhow::Error {
    let mut u = UserError::new(
        format!(
            "the content server refused to delete the existing scenes in {world} (HTTP {status})"
        ),
        TrySteps::one(
            "drop --replace-world-scenes to deploy alongside existing scenes without deleting them",
        )
        .and("check the signing wallet has permission on the world"),
    );
    let body = body.trim();
    if !body.is_empty() {
        u = u.why(body);
    }
    u.into()
}

pub async fn send_world_delete(target: &str, world: &str, chain: &serde_json::Value) -> Result<()> {
    let (status, body) = world_delete_request(target, world, chain).await?;
    if (200..300).contains(&status) {
        ux::note(format!(
            "removed the existing scenes in {world} (HTTP {status})"
        ));
        Ok(())
    } else {
        Err(world_delete_refused(world, status, &body))
    }
}

pub(super) async fn delete_world_scenes(target: &str, world: &str, wallet: &Wallet) -> Result<()> {
    let payload = build_delete_payload(world);
    let chain = catalyrst_crypto::create_simple_auth_chain(wallet, &payload)
        .context("EIP-191 sign of the scene-removal payload")?;
    let (status, body) = world_delete_request(target, world, &chain).await?;
    if (200..300).contains(&status) {
        ux::note(format!(
            "removed the existing scenes in {world} (HTTP {status})"
        ));
        return Ok(());
    }
    if status == 404 || status == 405 {
        return delete_scenes_per_coord(target, world, wallet).await;
    }
    Err(world_delete_refused(world, status, &body))
}

async fn delete_scenes_per_coord(target: &str, world: &str, wallet: &Wallet) -> Result<()> {
    let scenes = fetch_world_scenes(target, world)
        .await
        .context("listing the world scenes for per-scene removal")?;
    let client = upload_client()?;
    let mut removed = 0usize;
    for scene in &scenes {
        let Some(parcel) = scene.parcels.first() else {
            continue;
        };
        let suffix = format!("/world/{}/scenes/{parcel}", encode_segment(world));
        let path = format!("{}{suffix}", url_path(target));
        let url = format!("{target}{suffix}");
        let mut req = client.delete(&url);
        for (k, v) in crate::world::signed_headers(wallet, "delete", &path)? {
            req = req.header(k, v);
        }
        let resp = match req.send().await {
            Ok(resp) => resp,
            Err(e) => return Err(unreachable_server(&url, e)),
        };
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(world_delete_refused(world, status.as_u16(), &body));
        }
        removed += 1;
    }
    ux::note(format!(
        "removed {removed} existing scene(s) in {world} via the per-scene route"
    ));
    Ok(())
}

/// A delegated identity's auth chain: the wallet's SIGNER link, the
/// ephemeral delegation it signed once (`ECDSA_EPHEMERAL`), and the entity
/// signed by that ephemeral key. The content server verifies the delegation
/// and the entity signature against it — the wallet itself never touches
/// this deploy.
pub fn ephemeral_auth_chain(
    signer: &str,
    delegation_payload: &str,
    delegation_signature: &str,
    entity_id: &str,
    entity_signature: &str,
) -> serde_json::Value {
    json!([
        { "type": "SIGNER", "payload": signer, "signature": "" },
        { "type": "ECDSA_EPHEMERAL", "payload": delegation_payload, "signature": delegation_signature },
        { "type": "ECDSA_SIGNED_ENTITY", "payload": entity_id, "signature": entity_signature },
    ])
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
    upload_entity_with_chain(target, entity_id, entity_bytes, files, address, auth_chain).await
}

/// Upload with a pre-built auth chain of any length — the multipart
/// `authChain[i]` fields are derived from the chain, so a two-link wallet
/// deploy and a three-link ephemeral deploy travel the same path.
pub async fn upload_entity_with_chain(
    target: &str,
    entity_id: &str,
    entity_bytes: Vec<u8>,
    files: &[(String, String, Vec<u8>)],
    address: &str,
    auth_chain: serde_json::Value,
) -> Result<String> {
    let url = format!("{}/entities", target.trim_end_matches('/'));
    tracing::info!("uploading to {url} as {address} (entity {entity_id})");
    ux::note(format!("uploading to {url} as {address}"));

    // Node carries the upload, then curl, then reqwest. A Cloudflare-fronted
    // worlds server challenges reqwest and curl but not Node — the official
    // tooling is Node, so its fingerprint is the one the edge accepts. This
    // is only reliable because the deploy makes no request to the content
    // server before it: a reqwest pre-flight would flag the IP and the upload
    // that follows would inherit the challenge.
    let carried = match node_upload(&url, entity_id, &entity_bytes, files, &auth_chain).await {
        Some(r) => Some(r),
        None => curl_upload(&url, entity_id, &entity_bytes, files, &auth_chain).await,
    };
    let (status, body) = match carried {
        Some(Ok((c, b))) => (c, b),
        Some(Err(e)) => return Err(e),
        None => {
            let mut form = reqwest::multipart::Form::new()
                .text("entityId", entity_id.to_string())
                .text("authChain", serde_json::to_string(&auth_chain)?);
            for (i, link) in auth_chain.as_array().into_iter().flatten().enumerate() {
                let f = |k: &str| {
                    link.get(k)
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                };
                form = form
                    .text(format!("authChain[{i}][type]"), f("type"))
                    .text(format!("authChain[{i}][payload]"), f("payload"))
                    .text(format!("authChain[{i}][signature]"), f("signature"));
            }
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
            let resp = match upload_client()?.post(&url).multipart(form).send().await {
                Ok(resp) => resp,
                Err(e) => return Err(unreachable_server(&url, e)),
            };
            let s = resp.status().as_u16();
            (s, resp.text().await.unwrap_or_default())
        }
    };

    if status == 0 {
        // curl reached no server (connection refused, DNS failure, timeout):
        // no HTTP response, so `-w %{http_code}` prints 000. Same sentence
        // the reqwest transport error gives.
        return Err(UserError::new(
            "could not reach the content server",
            TrySteps::one("check the server is running and the URL is right").and(
                "targets: --target <catalyst-domain>, --target-content <content-server-url> (e.g. a local worlds server on http://127.0.0.1:5142)",
            ),
        )
        .why(format!("no response from {url}"))
        .into());
    }
    if (200..300).contains(&status) {
        tracing::info!("deployed \u{2713} (HTTP {status}) — server: {body}");
        Ok(format!("Deployed {entity_id} (HTTP {status})"))
    } else {
        Err(rejected(status, &body, &[]))
    }
}

pub fn play_url(world: Option<&str>, base: &str) -> String {
    match world {
        Some(w) => format!("https://decentraland.org/play/?realm={w}"),
        None => format!("https://play.decentraland.org/?NETWORK=mainnet&position={base}"),
    }
}

pub fn jump_in_url(world: Option<&str>, base: &str) -> String {
    format!("jump in: {}", play_url(world, base))
}

pub(crate) fn unreachable_server(url: &str, e: reqwest::Error) -> anyhow::Error {
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

/// An HTML challenge page is the EDGE answering, not the content server:
/// Cloudflare's bot protection intercepts the upload and hands back a
/// browser puzzle no upload client can solve. Recognising it turns a
/// screenful of dumped markup into the actual problem and its remedy.
fn edge_challenge(body: &str) -> bool {
    let b = body.trim_start();
    (b.starts_with("<!DOCTYPE") || b.starts_with("<html") || b.starts_with("<!--"))
        && (body.contains("Cloudflare") || body.contains("cf-ray") || body.contains("cf_chl"))
}

/// The content server refused a World because it is a Genesis City catalyst
/// (ADR-173): worlds and parcels are different destinations, and this scene
/// went to the wrong one. Almost always a `--target-content` /
/// `DCL_ONE_SDK_DEFAULT_TARGET` pointed at a Genesis catalyst while the
/// scene declares a `worldConfiguration`.
fn world_at_genesis(body: &str) -> bool {
    body.contains("ADR-173")
        || (body.contains("worldConfiguration") && body.contains("Genesis City"))
}

fn rejected(code: u16, body: &str, pointers: &[String]) -> anyhow::Error {
    if world_at_genesis(body) {
        return UserError::new(
            "this scene is a World, but it was sent to a Genesis City content server, which only takes parcel scenes",
            TrySteps::one(format!(
                "to publish the World: drop --target-content / DCL_ONE_SDK_DEFAULT_TARGET so it routes to the worlds server ({WORLDS_CONTENT_SERVER}), or point the target at a worlds server"
            ))
            .and("to deploy to Genesis City parcels instead: remove worldConfiguration from scene.json (the /target page's \"Point at Genesis City LAND\" does this)"),
        )
        .why("worlds and Genesis parcels are different deploy destinations; this target is a Genesis catalyst")
        .into();
    }
    if edge_challenge(body) {
        return UserError::new(
            format!(
                "the realm's edge challenged this deployment (HTTP {code}) \u{2014} it never reached the content server"
            ),
            TrySteps::one(
                "ask the realm operator to exempt POST …/entities from the edge's bot protection (a Cloudflare WAF skip rule), or to serve deploys on a DNS-only host",
            )
            .and("nothing was published, so retrying after the edge change is safe"),
        )
        .why("the answer was an HTML browser challenge, which an upload client cannot solve")
        .into();
    }
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

/// `DCL_ONE_SDK_DEFAULT_TARGET` is process-global state: every test that sets
/// it or renders through target resolution serializes on this lock, the
/// deploy-page tests included.
#[cfg(test)]
pub(crate) static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
mod tests {
    use super::*;

    /// A delegated deploy carries three links the content server can verify
    /// without the wallet: the SIGNER names it, the ECDSA_EPHEMERAL is the
    /// delegation it signed, and the ephemeral key signs the entity. The
    /// ephemeral signature actually recovers to the ephemeral address.
    #[test]
    fn an_ephemeral_chain_is_signer_delegation_and_entity() {
        let ephemeral = catalyrst_crypto::Wallet::from_hex(
            "0x0000000000000000000000000000000000000000000000000000000000000042",
        )
        .unwrap();
        let entity_id = "bafkreieexampleentityid";
        let entity_sig = ephemeral.sign_message(entity_id.as_bytes()).unwrap();
        let chain = ephemeral_auth_chain(
            "0xWALLET",
            "Decentraland Login\nEphemeral address: x\nExpiration: y",
            "0xdelegationsig",
            entity_id,
            &entity_sig,
        );
        let links = chain.as_array().unwrap();
        assert_eq!(links.len(), 3);
        assert_eq!(links[0]["type"], "SIGNER");
        assert_eq!(links[0]["payload"], "0xWALLET");
        assert_eq!(links[0]["signature"], "");
        assert_eq!(links[1]["type"], "ECDSA_EPHEMERAL");
        assert_eq!(links[1]["signature"], "0xdelegationsig");
        assert_eq!(links[2]["type"], "ECDSA_SIGNED_ENTITY");
        assert_eq!(links[2]["payload"], entity_id);
        // The entity link's signature is the ephemeral's, recoverable to it.
        let recovered = catalyrst_crypto::recover::recover_address(
            entity_id.as_bytes(),
            links[2]["signature"].as_str().unwrap(),
        )
        .unwrap();
        assert!(recovered.eq_ignore_ascii_case(&ephemeral.address()));
    }

    /// A Cloudflare challenge is the edge talking, not the catalyst: the
    /// error names the edge and the remedy instead of dumping a screenful
    /// of browser-challenge HTML, while a real server refusal keeps its
    /// body — a JSON error from the catalyst is worth reading verbatim.
    #[test]
    fn a_cloudflare_challenge_reads_as_the_edge_not_the_server() {
        let challenge = "<!DOCTYPE html>\n<html><head><title>Attention Required! | Cloudflare</title></head></html>";
        let e = crate::ux::render(&rejected(403, challenge, &[]), false, false);
        assert!(
            e.contains("the realm's edge challenged this deployment"),
            "{e}"
        );
        assert!(!e.contains("<!DOCTYPE"), "no markup dump: {e}");
        assert!(
            e.contains("bot protection"),
            "the remedy names the cause: {e}"
        );

        let server = r#"{"error":"address has no permission"}"#;
        let e = crate::ux::render(&rejected(403, server, &[]), false, false);
        assert!(
            e.contains("the content server rejected") && e.contains("no permission"),
            "a real refusal keeps its body: {e}"
        );
    }

    /// ADR-173 (a World sent to a Genesis catalyst) reads as the routing
    /// mistake it is — name the two destinations and the fix — not the raw
    /// server sentence.
    #[test]
    fn a_world_at_a_genesis_catalyst_names_the_routing_fix() {
        let body = r#"{"errors":["The scene.json contains a worldConfiguration section, which is not allowed for Genesis City scenes (see ADR-173: http://adr.decentraland.org/adr/ADR-173). Please remove it and try again."]}"#;
        let e = crate::ux::render(&rejected(400, body, &[]), false, false);
        assert!(e.contains("this scene is a World"), "{e}");
        assert!(
            e.contains("worlds server"),
            "the remedy points at the worlds server: {e}"
        );
        assert!(
            e.contains("Point at Genesis City LAND"),
            "and the other way out: {e}"
        );
        assert!(
            !e.contains("ADR-173"),
            "the raw server sentence is not the headline: {e}"
        );
    }

    async fn resolved(raw: Option<&str>, world: Option<&str>, headless: bool) -> Result<String> {
        match raw {
            Some(raw) => std::env::set_var("DCL_ONE_SDK_DEFAULT_TARGET", raw),
            None => std::env::remove_var("DCL_ONE_SDK_DEFAULT_TARGET"),
        }
        let out = resolve_target_from(None, None, world, headless, TargetConsent::default()).await;
        std::env::remove_var("DCL_ONE_SDK_DEFAULT_TARGET");
        out
    }

    /// A blank `DCL_ONE_SDK_DEFAULT_TARGET` used to sanitize to a bare
    /// "https:" and be deployed to. The landing page already reads a blank
    /// value as unset when it prints the deploy command, so every reader has
    /// to fall through the same way or the printed command and the real
    /// target disagree.
    #[tokio::test]
    async fn a_blank_default_target_env_is_unset_on_worlds_and_land() {
        let _guard = ENV_LOCK.lock().await;
        for raw in ["", "   "] {
            let world = resolved(Some(raw), Some("my.dcl.eth"), false).await;
            assert_eq!(
                world.expect("blank env falls through to the worlds default"),
                WORLDS_CONTENT_SERVER,
                "{raw:?}"
            );

            let land = resolved(Some(raw), None, true).await;
            let err = format!(
                "{:#}",
                land.expect_err("blank env must not become a target")
            );
            assert!(err.contains("no deploy target given"), "{raw:?}: {err}");

            std::env::set_var("DCL_ONE_SDK_DEFAULT_TARGET", raw);
            let worlds_server = crate::world::resolve_target(None);
            std::env::remove_var("DCL_ONE_SDK_DEFAULT_TARGET");
            assert_eq!(
                worlds_server.expect("blank env falls through to the worlds default"),
                WORLDS_CONTENT_SERVER,
                "{raw:?}"
            );
        }
    }

    /// Headless (key-signed) world deploys get the default too: the "never
    /// pick a server implicitly" rule exists so a key never uploads to an
    /// arbitrary public catalyst, but a world's canonical server is not
    /// arbitrary — `worldConfiguration.name` in scene.json already names the
    /// destination.
    #[tokio::test]
    async fn a_world_scene_defaults_to_the_public_worlds_server() {
        let _guard = ENV_LOCK.lock().await;
        for headless in [false, true] {
            let target = resolved(None, Some("gather.dcl.eth"), headless).await;
            assert_eq!(target.unwrap(), WORLDS_CONTENT_SERVER, "{headless}");
        }
        std::env::remove_var("DCL_ONE_SDK_DEFAULT_TARGET");
        assert_eq!(
            crate::world::resolve_target(None).unwrap(),
            WORLDS_CONTENT_SERVER
        );
    }

    #[tokio::test]
    async fn the_env_default_outranks_the_worlds_default() {
        let _guard = ENV_LOCK.lock().await;
        let target = resolved(Some("http://127.0.0.1:9"), Some("gather.dcl.eth"), true).await;
        assert_eq!(target.unwrap(), "http://127.0.0.1:9");
    }

    #[tokio::test]
    async fn an_explicit_target_content_outranks_the_worlds_default_and_the_env() {
        let _guard = ENV_LOCK.lock().await;
        std::env::set_var("DCL_ONE_SDK_DEFAULT_TARGET", "http://127.0.0.1:9");
        let target = resolve_target_from(
            None,
            Some("https://example.org/"),
            Some("gather.dcl.eth"),
            true,
            TargetConsent::default(),
        )
        .await;
        std::env::remove_var("DCL_ONE_SDK_DEFAULT_TARGET");
        assert_eq!(target.unwrap(), "https://example.org");
    }

    /// The exact shape `unpublish.rs` resolves with. Unpublish is land-only —
    /// a parcel never names a world — so the worlds default must not leak in
    /// and the key-signed refusal stays.
    #[tokio::test]
    async fn land_unpublish_resolution_still_refuses_without_a_target() {
        let _guard = ENV_LOCK.lock().await;
        let out = resolved(None, None, true).await;
        let err = format!("{:#}", out.expect_err("land + key must still refuse"));
        assert!(err.contains("no deploy target given"), "{err}");
    }
}
