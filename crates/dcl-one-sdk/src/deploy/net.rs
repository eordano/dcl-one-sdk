use super::{
    catalyst_rotation, configured_catalyst_rotation, now_ms, DeployOptions, UPSTREAM_CATALYST_HOSTS,
};
use crate::ux::{self, TrySteps, UserError};
use anyhow::{bail, Context, Result};
use catalyrst_crypto::Wallet;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::time::Duration;

/// Where a world deploy goes when no flag or env default names a server.
pub const WORLDS_CONTENT_SERVER: &str = "https://worlds-content-server.decentraland.org";

const USER_AGENT: &str = concat!("dcl-one-sdk/", env!("CARGO_PKG_VERSION"));
pub(crate) const VERBOSE_HINT: &str = "re-run with --verbose for the full response";

type Files = [(String, String, Vec<u8>)];

pub(crate) fn client(connect: Duration, total: Duration) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        // Honest identification: reqwest sends no User-Agent by default, and
        // an anonymous client scores as junk with every WAF between here and
        // a self-hosted realm. Saying who we are is not a bypass — an edge
        // that challenges still challenges — it just stops the requests
        // reading as nobody's.
        .user_agent(USER_AGENT)
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

/// Send and read the body; the caller maps the transport error to its own
/// "unreachable" message.
pub(crate) async fn send_text(req: reqwest::RequestBuilder) -> reqwest::Result<(u16, String)> {
    let resp = req.send().await?;
    Ok((
        resp.status().as_u16(),
        resp.text().await.unwrap_or_default(),
    ))
}

pub(crate) fn with_headers(
    req: reqwest::RequestBuilder,
    headers: Vec<(String, String)>,
) -> reqwest::RequestBuilder {
    headers.into_iter().fold(req, |r, (k, v)| r.header(k, v))
}

pub(crate) fn read_server_message() -> TrySteps {
    TrySteps::one("read the server message above").and(VERBOSE_HINT)
}

/// A refusal carrying the server body as its "why" when there is one.
pub(crate) fn refusal(u: UserError, body: &str) -> anyhow::Error {
    let body = body.trim();
    if body.is_empty() {
        u.into()
    } else {
        u.why(body).into()
    }
}

fn stage_entity(dir: &Path, entity_id: &str, entity_bytes: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(entity_id), entity_bytes)
}

/// The multipart `authChain[i][k]` fields, one per link and key.
fn chain_fields(chain: &Value) -> Vec<(usize, &'static str, &str)> {
    let mut out = Vec::new();
    for (i, link) in chain.as_array().into_iter().flatten().enumerate() {
        for k in ["type", "payload", "signature"] {
            out.push((i, k, link.get(k).and_then(Value::as_str).unwrap_or("")));
        }
    }
    out
}

/// Upload with Node's `fetch`: a Cloudflare-fronted worlds server challenges
/// reqwest and curl but not Node, whose fingerprint (the official Creator Hub
/// and sdk-commands) is the one the edge accepts. `None` means Node is
/// absent.
async fn node_upload(
    url: &str,
    entity_id: &str,
    entity_bytes: &[u8],
    files: &Files,
    auth_chain: &Value,
) -> Option<Result<(u16, String)>> {
    let node = crate::build::find_node()?;
    let dir = std::env::temp_dir().join(format!("dcl-one-sdk-nodeup-{entity_id}"));
    let stage = || -> std::io::Result<()> {
        stage_entity(&dir, entity_id, entity_bytes)?;
        for (_, hash, bytes) in files {
            std::fs::write(dir.join(hash), bytes)?;
        }
        let cfg = json!({
            "url": url,
            "dir": dir.to_string_lossy(),
            "entityId": entity_id,
            "authChain": auth_chain,
            "files": files.iter().map(|(_, h, _)| h).collect::<Vec<_>>(),
        });
        std::fs::write(dir.join("cfg.json"), serde_json::to_vec(&cfg)?)?;
        std::fs::write(dir.join("up.mjs"), NODE_UPLOAD_MJS)
    };
    if stage().is_err() {
        let _ = std::fs::remove_dir_all(&dir);
        return Some(Err(anyhow::anyhow!("could not stage the upload")));
    }
    let out = tokio::process::Command::new(&node)
        .arg(dir.join("up.mjs"))
        .arg(dir.join("cfg.json"))
        .output()
        .await;
    let _ = std::fs::remove_dir_all(&dir);
    let o = match out {
        Ok(o) => o,
        Err(e) => return Some(Err(anyhow::anyhow!("node could not run: {e}"))),
    };
    let stdout = String::from_utf8_lossy(&o.stdout);
    // No JSON on stdout means the fetch threw before a response —
    // a connection refused / DNS failure / timeout. Report it as
    // status 0, which the caller renders as "could not reach the
    // content server", the same as the reqwest transport error.
    let Ok(v) = serde_json::from_str::<Value>(stdout.trim()) else {
        return Some(Ok((0, String::new())));
    };
    let code = v.get("status").and_then(Value::as_u64).unwrap_or(0) as u16;
    let body = v
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Some(Ok((code, body)))
}

/// Node 18+ has global `fetch`/`FormData`/`Blob`; prints one JSON line with
/// the status and body.
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

/// Upload with curl, the fallback when Node is absent: its multipart
/// fingerprint passes edges that challenge reqwest's. `None` means curl is
/// absent too.
async fn curl_upload(
    url: &str,
    entity_id: &str,
    entity_bytes: &[u8],
    files: &Files,
    auth_chain: &Value,
) -> Option<Result<(u16, String)>> {
    let dir = std::env::temp_dir().join(format!("dcl-one-sdk-upload-{entity_id}"));
    if stage_entity(&dir, entity_id, entity_bytes).is_err() {
        return Some(Err(anyhow::anyhow!("could not stage the upload")));
    }
    let part = |name: &str, mime: &str| {
        format!(
            "{name}=@{};type={mime};filename={name}",
            dir.join(name).display()
        )
    };
    let mut cmd = tokio::process::Command::new("curl");
    cmd.args([
        "-sS",
        "-X",
        "POST",
        "-A",
        USER_AGENT,
        "-w",
        "\n%{http_code}",
    ])
    .arg("-F")
    .arg(format!("entityId={entity_id}"))
    .arg("-F")
    .arg(format!(
        "authChain={}",
        serde_json::to_string(auth_chain).ok()?
    ));
    for (i, k, v) in chain_fields(auth_chain) {
        cmd.arg("-F").arg(format!("authChain[{i}][{k}]={v}"));
    }
    cmd.arg("-F").arg(part(entity_id, "application/json"));
    for (_, hash, bytes) in files {
        if std::fs::write(dir.join(hash), bytes).is_err() {
            let _ = std::fs::remove_dir_all(&dir);
            return Some(Err(anyhow::anyhow!("could not stage a payload file")));
        }
        cmd.arg("-F").arg(part(hash, "application/octet-stream"));
    }
    cmd.arg(url);
    let out = cmd.output().await;
    let _ = std::fs::remove_dir_all(&dir);
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

async fn reqwest_upload(
    url: &str,
    entity_id: &str,
    entity_bytes: Vec<u8>,
    files: &Files,
    auth_chain: &Value,
) -> Result<(u16, String)> {
    use reqwest::multipart::{Form, Part};
    let mut form = Form::new()
        .text("entityId", entity_id.to_string())
        .text("authChain", serde_json::to_string(auth_chain)?);
    for (i, k, v) in chain_fields(auth_chain) {
        form = form.text(format!("authChain[{i}][{k}]"), v.to_string());
    }
    form = form.part(
        entity_id.to_string(),
        Part::bytes(entity_bytes)
            .file_name(entity_id.to_string())
            .mime_str("application/json")?,
    );
    for (_, hash, bytes) in files {
        form = form.part(
            hash.clone(),
            Part::bytes(bytes.clone()).file_name(hash.clone()),
        );
    }
    send_text(upload_client()?.post(url).multipart(form))
        .await
        .map_err(|e| unreachable_server(url, e))
}

/// How far the caller has already consented to a target being chosen for it —
/// the only path on which this CLI reaches the public network unasked.
#[derive(Clone, Copy, Default)]
pub(super) struct TargetConsent {
    pub assume_yes: bool,
    pub non_interactive: bool,
    /// Skip the chosen-for-you note: the page-driven flow names the
    /// destination on the page that consented to it.
    pub quiet: bool,
}

impl TargetConsent {
    fn from_opts(opts: &DeployOptions) -> Self {
        TargetConsent {
            assume_yes: opts.yes,
            non_interactive: opts.ci,
            quiet: opts.quiet,
        }
    }
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
        TargetConsent::from_opts(opts),
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
    let base = match (target, target_content) {
        (Some(_), Some(_)) => {
            return Err(UserError::new(
                "pass the target once: --target-content is an alias of --target-server",
                TrySteps::one("--target-server <catalyst domain or content-server URL>"),
            )
            .into())
        }
        (None, Some(tc)) => tc.trim_end_matches('/').to_string(),
        (Some(t), None) => target_content_url(t, "--target-server").await?,
        (None, None) => match (configured_target_server(), world) {
            (Some(t), _) => target_content_url(&t, "DCL_ONE_SDK_TARGET_SERVER").await?,
            (None, Some(w)) => {
                ux::note(format!(
                    "deploying the world \"{w}\" to the public worlds server {WORLDS_CONTENT_SERVER}"
                ));
                DEFAULT_WORLDS_TARGET_SERVER.to_string()
            }
            (None, None) if headless => return Err(UserError::new(
                "no deploy target given for key-based signing",
                TrySteps::one("pass --target-server <catalyst domain or content-server URL>")
                    .and("or set DCL_ONE_SDK_TARGET_SERVER for this run")
                    .and("browser signing (no key) picks a healthy public catalyst automatically"),
            )
            .why("key-signed deploys never pick a server implicitly")
            .into()),
            (None, None) => rotation_content_url(consent).await?,
        },
    };
    check_target_kind(&base, world)?;
    Ok(base)
}

/// The two public destinations take different scenes, and both refuse the
/// wrong kind only after the build and the wallet prompt: known by host, the
/// mismatch is refused here instead, with the same words the upload's own
/// refusal would use. A self-hosted realm is not known by host and keeps
/// the upload refusal as its backstop.
fn check_target_kind(base: &str, world: Option<&str>) -> Result<()> {
    let host = host_of(base);
    let at_worlds = host.is_some() && host == host_of(WORLDS_CONTENT_SERVER);
    let at_genesis = UPSTREAM_CATALYST_HOSTS
        .iter()
        .any(|u| host.is_some() && host == host_of(u));
    match (world, at_worlds, at_genesis) {
        (None, true, _) => Err(super::world_gate::refuse_plain_scene_at_worlds()),
        (Some(_), _, true) => Err(super::world_gate::refuse_world_at_genesis()),
        _ => Ok(()),
    }
}

pub fn non_upstream_note(target: &str) -> Option<String> {
    let host = host_of(target)?;
    let upstream = std::iter::once(super::DEFAULT_GENESIS_TARGET_SERVER)
        .chain(UPSTREAM_CATALYST_HOSTS.iter().copied())
        .any(|r| host_of(r).is_some_and(|h| h.eq_ignore_ascii_case(&host)));
    (!upstream).then(|| {
        format!(
            "publishing to {host}: this updates that network only, not Genesis City on decentraland.org"
        )
    })
}

fn after_scheme(url: &str) -> &str {
    url.split_once("://").map_or(url, |(_, r)| r)
}

pub(crate) fn host_of(url: &str) -> Option<String> {
    let host = after_scheme(url)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    (!host.is_empty()).then(|| host.to_string())
}

pub(super) fn url_path(base: &str) -> String {
    let rest = after_scheme(base);
    rest.find('/')
        .map_or(String::new(), |i| rest[i..].to_string())
}

/// This build's own default worlds server: `WORLDS_CONTENT_SERVER` under
/// its own name, so a fork wanting a different one (its own worlds server)
/// has a single constant to edit, matching `DEFAULT_GENESIS_TARGET_SERVER`.
pub const DEFAULT_WORLDS_TARGET_SERVER: &str = WORLDS_CONTENT_SERVER;

/// Set from `DCL_ONE_SDK_TARGET_SERVER`. A blank value is "unset", not the
/// empty string: it would sanitize to a bare "https:". The landing page
/// reads it the same way when it prints the deploy command, so both fall
/// through together.
pub fn configured_target_server() -> Option<String> {
    std::env::var("DCL_ONE_SDK_TARGET_SERVER")
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Deploy targets are not remembered: the scene names its destination, and
/// `--target-server` or `DCL_ONE_SDK_TARGET_SERVER` speak for one run only. Older
/// releases wrote `.dcl-one/deploy-target` from the variable and re-adopted
/// it on every run since; a leftover is removed, with a note, so it can
/// never route a deploy again.
pub fn forget_remembered_target(root: &Path) {
    let path = root.join(".dcl-one").join("deploy-target");
    if path.exists() {
        let _ = std::fs::remove_file(&path);
        ux::note(format!(
            "removed {} \u{2014} deploy targets are no longer remembered; pass --target-server, or set DCL_ONE_SDK_TARGET_SERVER for one run",
            path.display()
        ));
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

async fn get_json(client: &reqwest::Client, url: &str, parsing: String) -> Result<Value> {
    let resp = client.get(url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        bail!("GET {url} returned HTTP {}", status.as_u16());
    }
    resp.json::<Value>().await.context(parsing)
}

async fn fetch_about(client: &reqwest::Client, base: &str) -> Result<Value> {
    let url = format!("{base}/about");
    get_json(client, &url, format!("parsing {url} as JSON")).await
}

fn about_content_url(about: &Value, base: &str) -> Option<String> {
    let u = about.get("content")?.get("publicUrl")?.as_str()?;
    let u = u.trim_end_matches('/');
    Some(if u.contains("://") {
        u.to_string()
    } else {
        format!("{base}{u}")
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
                    .and("for a content server, pass its URL with the scheme, e.g. --target-server https://host/content"),
            )
            .caused_by(std::io::Error::other(format!("{e:#}"))),
        )
    })?;
    about_content_url(&about, &base).ok_or_else(|| {
        UserError::new(
            format!("the catalyst {base} did not report a content server"),
            TrySteps::one("check <domain>/about returns content.publicUrl")
                .and("for a content server, pass its URL with the scheme, e.g. --target-server https://host/content"),
        )
        .into()
    })
}

/// One target, its shape decides: a domain with no path — bare, or with a
/// scheme and nothing after the host — is a catalyst whose /about names the
/// content server; a URL that already has a path is that content server,
/// used verbatim. `https://peer.decentraland.org` still discovers, exactly
/// as a plain `--target <domain>` always has; only a value that already
/// spells out where the entities route lives skips the probe. The public
/// worlds server is verbatim by name regardless of path — a /about probe
/// against its Cloudflare edge gets the following upload challenged, and
/// that probe was the reason two flags once existed.
async fn target_content_url(t: &str, source: &str) -> Result<String> {
    let t = t.trim();
    let base = sanitize_catalyst_url(t);
    let worlds = host_of(&base) == host_of(WORLDS_CONTENT_SERVER);
    let has_path = !url_path(&base).is_empty();
    if !worlds && !has_path {
        return catalyst_content_url(t).await;
    }
    ux::note(format!("using {source} as a content server: {base}"));
    Ok(base)
}

/// Get a yes for a destructive or implicit step: `refuse` is the error when
/// nobody can be asked, `cancel_step` the way out named after a "no".
fn confirm(
    consent: TargetConsent,
    refuse: impl FnOnce() -> UserError,
    cancel_step: &str,
) -> Result<()> {
    if consent.assume_yes {
        return Ok(());
    }
    if consent.non_interactive || !std::io::stdin().is_terminal() {
        return Err(refuse().into());
    }
    if prompt_continue()? {
        Ok(())
    } else {
        Err(UserError::new("deployment cancelled", TrySteps::one(cancel_step)).into())
    }
}

fn consent_to_public_deploy(base: &str, consent: TargetConsent) -> Result<()> {
    let host = host_of(base).unwrap_or_else(|| base.to_string());
    if !consent.quiet {
        ux::note(format!(
            "no --target-server given \u{2014} publishing to the public Genesis City network via {host}"
        ));
    }
    confirm(
        consent,
        || {
            UserError::new(
                format!("this deploy would publish to the public Genesis City network via {base}"),
                TrySteps::one(
                    "pass --target-server <catalyst-domain> or --target-content <url> to publish elsewhere",
                )
                .and("or set DCL_ONE_SDK_TARGET_SERVER=<catalyst-or-content-url>")
                .and("or pass --yes to confirm the public deploy non-interactively"),
            )
            .why("no target was given, so the target was chosen for you")
        },
        "pass --target-server <catalyst-domain> to publish somewhere else",
    )
}

async fn rotation_content_url(consent: TargetConsent) -> Result<String> {
    let configured = configured_catalyst_rotation();
    let rotation = configured.clone().unwrap_or_else(catalyst_rotation);
    let client = probe_client()?;
    let probes: Vec<_> = rotation
        .iter()
        .map(|base| {
            let client = client.clone();
            let base = base.clone();
            tokio::spawn(async move {
                let about = fetch_about(&client, &base).await.ok()?;
                let healthy = about.get("healthy").and_then(Value::as_bool);
                healthy
                    .unwrap_or(false)
                    .then(|| about_content_url(&about, &base))
                    .flatten()
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
            .and("or pass --target-server <catalyst-domain> / --target-content <url> explicitly"),
    )
    .into())
}

pub struct WorldScene {
    pub title: String,
    pub parcels: Vec<String>,
    pub timestamp: Option<i64>,
    pub content_hashes: Vec<String>,
    /// Deployed bytes; some servers report `size` as a stringified integer,
    /// others not at all.
    pub size: Option<u64>,
}

pub(crate) fn entity_title(entity: &Value) -> String {
    entity
        .get("metadata")
        .and_then(|m| m.get("display"))
        .and_then(|d| d.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("Untitled")
        .to_string()
}

fn string_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn entity_content_hashes(entity: &Value) -> Vec<String> {
    entity
        .get("content")
        .and_then(Value::as_array)
        .map(|content| {
            content
                .iter()
                .filter_map(|f| f.get("hash").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn parse_world_scenes(body: &Value) -> Vec<WorldScene> {
    body.get("scenes")
        .and_then(Value::as_array)
        .map(|scenes| {
            scenes
                .iter()
                .map(|s| {
                    let entity = s.get("entity").cloned().unwrap_or_default();
                    WorldScene {
                        title: entity_title(&entity),
                        parcels: string_list(s.get("parcels")),
                        timestamp: entity.get("timestamp").and_then(Value::as_i64),
                        content_hashes: entity_content_hashes(&entity),
                        size: s.get("size").and_then(|v| match v {
                            Value::String(s) => s.parse().ok(),
                            other => other.as_u64(),
                        }),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn fetch_world_scenes(target: &str, world: &str) -> Result<Vec<WorldScene>> {
    let url = format!("{target}/world/{}/scenes", encode_segment(world));
    let body = get_json(
        &probe_client()?,
        &url,
        "parsing the world scenes list".to_string(),
    )
    .await?;
    Ok(parse_world_scenes(&body))
}

struct PermissionCheck {
    allowed: bool,
    denied_parcels: Vec<String>,
    /// So a refusal can show who owns the world and who was granted what.
    doc: Value,
}

/// Whether the signing wallet may publish to the world, asked before the
/// upload. Off unless `deploy --check-permissions`: the pre-flight GET is
/// what gets the upload challenged behind a Cloudflare-fronted worlds
/// server, and the server refuses an unpermitted upload itself. On, a
/// refusal names the owner and the grant command instead of a bare HTTP
/// error. Land deploys have no such document and always pass.
#[derive(Clone)]
pub struct PermissionGate {
    pub target: String,
    pub world: Option<String>,
    pub pointers: Vec<String>,
    pub enabled: bool,
}

impl PermissionGate {
    pub fn off() -> Self {
        PermissionGate {
            target: String::new(),
            world: None,
            pointers: Vec::new(),
            enabled: false,
        }
    }

    pub async fn verify(&self, address: &str) -> Result<()> {
        match (self.enabled, self.world.as_deref()) {
            (true, Some(world)) => {
                enforce_world_permission(&self.target, world, address, &self.pointers).await
            }
            _ => Ok(()),
        }
    }
}

/// What the permissions document alone says; only `NeedsParcels` pays for the
/// second (scoped-parcels) request. Pure, so the deploy and the /target page
/// provably decide alike.
pub(crate) enum DocAnswer {
    Granted,
    NeedsParcels,
}

pub(crate) fn deployment_permission_in_doc(doc: &Value, address: &str) -> DocAnswer {
    let same = |v: &Value| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case(address));
    if doc.get("owner").is_some_and(same) {
        return DocAnswer::Granted;
    }
    if let Some(dep) = doc.get("permissions").and_then(|p| p.get("deployment")) {
        if dep.get("type").and_then(Value::as_str) == Some("unrestricted") {
            return DocAnswer::Granted;
        }
        let in_wallets = dep
            .get("wallets")
            .and_then(Value::as_array)
            .is_some_and(|arr| arr.iter().any(same));
        if in_wallets {
            let world_wide = doc
                .get("summary")
                .and_then(|s| s.get(address.to_lowercase()))
                .and_then(Value::as_array)
                .and_then(|arr| {
                    arr.iter()
                        .find(|e| e.get("permission").and_then(Value::as_str) == Some("deployment"))
                })
                .map(|e| {
                    e.get("world_wide")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                });
            if world_wide.unwrap_or(true) {
                return DocAnswer::Granted;
            }
        }
    }
    DocAnswer::NeedsParcels
}

/// The deploying pointers the scoped-grant list does NOT cover.
pub(crate) fn denied_parcels_in(scoped: &Value, deploying: &[String]) -> Vec<String> {
    let allowed: HashSet<&str> = scoped
        .get("parcels")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
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
    let body = get_json(&client, &url, "parsing the world permissions".to_string()).await?;
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
    let parcel_body = get_json(&client, &url, "parsing the parcel permissions".to_string()).await?;
    let denied_parcels = denied_parcels_in(&parcel_body, deploying);
    Ok(PermissionCheck {
        allowed: denied_parcels.is_empty(),
        denied_parcels,
        doc: body,
    })
}

async fn enforce_world_permission(
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
                .and_then(Value::as_str)
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
    confirm(
        TargetConsent::from_opts(opts),
        || {
            UserError::new(
                format!(
                    "this deploy would delete {} existing scene(s) in {world}",
                    others.len()
                ),
                TrySteps::one(
                    "drop --replace-world-scenes to deploy alongside them (additive, the default)",
                )
                .and("or pass --yes to confirm the deletion non-interactively"),
            )
        },
        "drop --replace-world-scenes to deploy alongside the existing scenes",
    )?;
    Ok(true)
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

pub fn simple_auth_chain(address: &str, payload: &str, signature: &str) -> Value {
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

async fn world_delete_request(target: &str, world: &str, chain: &Value) -> Result<(u16, String)> {
    let payload = chain
        .as_array()
        .and_then(|links| links.last())
        .and_then(|l| l.get("payload"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let url = format!("{target}/entities/{}", encode_segment(world));
    let req = with_headers(
        upload_client()?.delete(&url),
        crate::world::headers_from_chain(payload, chain),
    );
    send_text(req)
        .await
        .map_err(|e| unreachable_server(&url, e))
}

fn world_delete_refused(world: &str, status: u16, body: &str) -> anyhow::Error {
    refusal(
        UserError::new(
            format!(
                "the content server refused to delete the existing scenes in {world} (HTTP {status})"
            ),
            TrySteps::one(
                "drop --replace-world-scenes to deploy alongside existing scenes without deleting them",
            )
            .and("check the signing wallet has permission on the world"),
        ),
        body,
    )
}

fn world_delete_outcome(world: &str, status: u16, body: &str) -> Result<()> {
    if !(200..300).contains(&status) {
        return Err(world_delete_refused(world, status, body));
    }
    ux::note(format!(
        "removed the existing scenes in {world} (HTTP {status})"
    ));
    Ok(())
}

pub async fn send_world_delete(target: &str, world: &str, chain: &Value) -> Result<()> {
    let (status, body) = world_delete_request(target, world, chain).await?;
    world_delete_outcome(world, status, &body)
}

pub(super) async fn delete_world_scenes(target: &str, world: &str, wallet: &Wallet) -> Result<()> {
    let payload = build_delete_payload(world);
    let chain = catalyrst_crypto::create_simple_auth_chain(wallet, &payload)
        .context("EIP-191 sign of the scene-removal payload")?;
    let (status, body) = world_delete_request(target, world, &chain).await?;
    if status == 404 || status == 405 {
        return delete_scenes_per_coord(target, world, wallet).await;
    }
    world_delete_outcome(world, status, &body)
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
        let headers = crate::world::signed_headers(wallet, "delete", &path)?;
        let (status, body) = send_text(with_headers(client.delete(&url), headers))
            .await
            .map_err(|e| unreachable_server(&url, e))?;
        if !(200..300).contains(&status) {
            return Err(world_delete_refused(world, status, &body));
        }
        removed += 1;
    }
    ux::note(format!(
        "removed {removed} existing scene(s) in {world} via the per-scene route"
    ));
    Ok(())
}

/// A delegated identity's chain: the wallet's SIGNER link, the ephemeral
/// delegation it signed once, and the entity signed by that ephemeral key.
pub fn ephemeral_auth_chain(
    signer: &str,
    delegation_payload: &str,
    delegation_signature: &str,
    entity_id: &str,
    entity_signature: &str,
) -> Value {
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
    files: &Files,
    address: &str,
    signature: &str,
) -> Result<String> {
    upload_entity_to(
        target,
        entity_id,
        entity_bytes,
        files,
        address,
        signature,
        UploadDestination::ContentServer,
    )
    .await
}

/// The human destination named in an upload's terminal headline. The content
/// server URL is still printed below it, once, as a copyable detail.
pub(crate) enum UploadDestination<'a> {
    ContentServer,
    World { name: &'a str, multi_scene: bool },
    Land { base: &'a str, parcels: usize },
}

impl UploadDestination<'_> {
    fn headline(&self) -> String {
        match self {
            UploadDestination::ContentServer => "⇡ uploading scene to content server".to_string(),
            UploadDestination::World {
                name,
                multi_scene: true,
            } => format!("⇡ uploading scene to multi-scene world {name}"),
            UploadDestination::World {
                name,
                multi_scene: false,
            } => format!("⇡ uploading scene to world {name}"),
            UploadDestination::Land { base, parcels } => match parcels {
                1 => format!("⇡ uploading scene to LAND at {base}"),
                n => format!("⇡ uploading scene to LAND at {base} ({n} parcels)"),
            },
        }
    }
}

pub(crate) async fn upload_entity_to(
    target: &str,
    entity_id: &str,
    entity_bytes: Vec<u8>,
    files: &Files,
    address: &str,
    signature: &str,
    destination: UploadDestination<'_>,
) -> Result<String> {
    let auth_chain = simple_auth_chain(address, entity_id, signature);
    upload_entity_with_chain_to(
        target,
        entity_id,
        entity_bytes,
        files,
        address,
        auth_chain,
        destination,
    )
    .await
}

pub(crate) async fn upload_entity_with_chain_to(
    target: &str,
    entity_id: &str,
    entity_bytes: Vec<u8>,
    files: &Files,
    address: &str,
    auth_chain: Value,
    destination: UploadDestination<'_>,
) -> Result<String> {
    let url = format!("{}/entities", target.trim_end_matches('/'));
    tracing::info!("uploading to {url} as {address} (entity {entity_id})");
    // Keep a publish legible beside watch events: the action owns the clock,
    // while the long URL and signer get their own continuation lines.
    ux::note_clocked(destination.headline());
    ux::note_arrow(format!("url: {url}"));
    ux::note_arrow(format!("signer: {address}"));

    // Node carries the upload, then curl, then reqwest. A Cloudflare-fronted
    // worlds server challenges reqwest and curl but not Node — the official
    // tooling is Node, so its fingerprint is the one the edge accepts. This
    // is only reliable because the deploy makes no request to the content
    // server before it: a reqwest pre-flight would flag the IP and the upload
    // that follows would inherit the challenge.
    let carried = match node_upload(&url, entity_id, &entity_bytes, files, &auth_chain).await {
        Some(r) => r,
        None => match curl_upload(&url, entity_id, &entity_bytes, files, &auth_chain).await {
            Some(r) => r,
            None => reqwest_upload(&url, entity_id, entity_bytes, files, &auth_chain).await,
        },
    };
    let (status, body) = carried?;

    if status == 0 {
        // curl reached no server (connection refused, DNS failure, timeout):
        // no HTTP response, so `-w %{http_code}` prints 000. Same sentence
        // the reqwest transport error gives.
        return Err(cannot_reach(format!("no response from {url}")).into());
    }
    if (200..300).contains(&status) {
        tracing::info!("deployed \u{2713} (HTTP {status}) — server: {body}");
        Ok(format!(
            "Deployed {entity_id} to {} (HTTP {status})",
            host_of(&url).unwrap_or_default()
        ))
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

fn cannot_reach(why: String) -> UserError {
    UserError::new(
        "could not reach the content server",
        TrySteps::one("check the server is running and the URL is right").and(
            "targets: --target-server <catalyst-domain>, --target-content <content-server-url> (e.g. a local worlds server on http://127.0.0.1:5142)",
        ),
    )
    .why(why)
}

pub(crate) fn unreachable_server(url: &str, e: reqwest::Error) -> anyhow::Error {
    let cause = if e.is_timeout() {
        "timed out"
    } else {
        classify_io(&e)
    };
    cannot_reach(format!("{cause}: {url}")).caused_by(e).into()
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

/// An HTML challenge page is Cloudflare's edge answering, not the content
/// server: a browser puzzle no upload client can solve.
fn edge_challenge(body: &str) -> bool {
    let b = body.trim_start();
    (b.starts_with("<!DOCTYPE") || b.starts_with("<html") || b.starts_with("<!--"))
        && (body.contains("Cloudflare") || body.contains("cf-ray") || body.contains("cf_chl"))
}

/// A World refused by a Genesis City catalyst (ADR-173): almost always a
/// `--target-content` / `DCL_ONE_SDK_TARGET_SERVER` pointed at the wrong
/// kind of server.
fn world_at_genesis(body: &str) -> bool {
    body.contains("ADR-173")
        || (body.contains("worldConfiguration") && body.contains("Genesis City"))
}

pub(super) fn rejected(code: u16, body: &str, pointers: &[String]) -> anyhow::Error {
    if super::world_gate::plain_scene_at_worlds(body) {
        return super::world_gate::refuse_plain_scene_at_worlds();
    }
    if world_at_genesis(body) {
        return super::world_gate::refuse_world_at_genesis();
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
        .and(VERBOSE_HINT)
    } else {
        read_server_message()
    };
    refusal(
        UserError::new(
            format!("the content server rejected this deployment (HTTP {code})"),
            steps,
        ),
        body,
    )
}

/// `DCL_ONE_SDK_TARGET_SERVER` is process-global: every test that sets it or
/// resolves a target serializes on this lock, the deploy-page tests included.
#[cfg(test)]
pub(crate) static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_headlines_name_the_scene_destination() {
        assert_eq!(
            UploadDestination::World {
                name: "arcade.dcl.eth",
                multi_scene: false,
            }
            .headline(),
            "⇡ uploading scene to world arcade.dcl.eth"
        );
        assert_eq!(
            UploadDestination::World {
                name: "arcade.dcl.eth",
                multi_scene: true,
            }
            .headline(),
            "⇡ uploading scene to multi-scene world arcade.dcl.eth"
        );
        assert_eq!(
            UploadDestination::Land {
                base: "2,12",
                parcels: 1,
            }
            .headline(),
            "⇡ uploading scene to LAND at 2,12"
        );
        assert_eq!(
            UploadDestination::Land {
                base: "2,12",
                parcels: 3,
            }
            .headline(),
            "⇡ uploading scene to LAND at 2,12 (3 parcels)"
        );
    }

    /// The ephemeral signature actually recovers to the ephemeral address.
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
        let recovered = catalyrst_crypto::recover::recover_address(
            entity_id.as_bytes(),
            links[2]["signature"].as_str().unwrap(),
        )
        .unwrap();
        assert!(recovered.eq_ignore_ascii_case(&ephemeral.address()));
    }

    fn rendered(code: u16, body: &str) -> String {
        crate::ux::render(&rejected(code, body, &[]), false, false)
    }

    /// A Cloudflare challenge names the edge and the remedy instead of dumping
    /// markup; a real server refusal keeps its body.
    #[test]
    fn a_cloudflare_challenge_reads_as_the_edge_not_the_server() {
        let e = rendered(403, "<!DOCTYPE html>\n<html><head><title>Attention Required! | Cloudflare</title></head></html>");
        assert!(
            e.contains("the realm's edge challenged this deployment"),
            "{e}"
        );
        assert!(!e.contains("<!DOCTYPE"), "no markup dump: {e}");
        assert!(
            e.contains("bot protection"),
            "the remedy names the cause: {e}"
        );

        let e = rendered(403, r#"{"error":"address has no permission"}"#);
        assert!(
            e.contains("the content server rejected") && e.contains("no permission"),
            "a real refusal keeps its body: {e}"
        );
    }

    #[test]
    fn a_world_at_a_genesis_catalyst_names_the_routing_fix() {
        let e = rendered(
            400,
            r#"{"errors":["The scene.json contains a worldConfiguration section, which is not allowed for Genesis City scenes (see ADR-173: http://adr.decentraland.org/adr/ADR-173). Please remove it and try again."]}"#,
        );
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

    fn set_env(raw: Option<&str>) {
        match raw {
            Some(raw) => std::env::set_var("DCL_ONE_SDK_TARGET_SERVER", raw),
            None => std::env::remove_var("DCL_ONE_SDK_TARGET_SERVER"),
        }
    }

    /// Nothing is remembered any more; a file an older release left behind
    /// is removed so it cannot route a deploy again.
    #[test]
    fn a_leftover_remembered_target_is_removed() {
        let root = std::env::temp_dir().join(format!("dcl-one-sdk-forget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".dcl-one")).unwrap();
        let file = root.join(".dcl-one").join("deploy-target");
        std::fs::write(&file, "worlds.example\n").unwrap();
        forget_remembered_target(&root);
        assert!(!file.exists());
        forget_remembered_target(&root);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The public worlds server is verbatim even when named bare; a URL with
    /// a scheme is verbatim; and a scene of the wrong kind is refused by host
    /// before anything is built.
    #[tokio::test]
    async fn the_target_shape_and_kind_decide() {
        let _guard = ENV_LOCK.lock().await;
        let bare_worlds = resolved(
            Some("worlds-content-server.decentraland.org"),
            Some("w.dcl.eth"),
            true,
        )
        .await;
        assert_eq!(
            bare_worlds.unwrap(),
            WORLDS_CONTENT_SERVER,
            "known worlds host, no /about probe"
        );

        let land_at_worlds = resolved(Some(WORLDS_CONTENT_SERVER), None, true).await;
        let err = format!(
            "{:#}",
            land_at_worlds.expect_err("a land scene at the worlds server")
        );
        assert!(err.contains("names no world"), "{err}");

        let world_at_genesis = resolve_target_from(
            Some("https://peer-ec1.decentraland.org/content"),
            None,
            Some("w.dcl.eth"),
            true,
            TargetConsent::default(),
        )
        .await;
        let err = format!(
            "{:#}",
            world_at_genesis.expect_err("a world at a Genesis catalyst")
        );
        assert!(err.contains("Genesis City content server"), "{err}");
    }

    async fn resolved(raw: Option<&str>, world: Option<&str>, headless: bool) -> Result<String> {
        set_env(raw);
        let out = resolve_target_from(None, None, world, headless, TargetConsent::default()).await;
        set_env(None);
        out
    }

    fn worlds_resolved(raw: Option<&str>) -> Result<String> {
        set_env(raw);
        let out = crate::world::resolve_target(None);
        set_env(None);
        out
    }

    /// The landing page reads a blank value as unset when it prints the
    /// deploy command; every reader must fall through the same way.
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

            assert_eq!(
                worlds_resolved(Some(raw)).expect("blank env falls through to the worlds default"),
                WORLDS_CONTENT_SERVER,
                "{raw:?}"
            );
        }
    }

    /// Headless world deploys get the default too: "never pick a server
    /// implicitly" guards keys against arbitrary catalysts, but
    /// `worldConfiguration.name` already names the destination.
    #[tokio::test]
    async fn a_world_scene_defaults_to_the_public_worlds_server() {
        let _guard = ENV_LOCK.lock().await;
        for headless in [false, true] {
            let target = resolved(None, Some("gather.dcl.eth"), headless).await;
            assert_eq!(target.unwrap(), WORLDS_CONTENT_SERVER, "{headless}");
        }
        assert_eq!(worlds_resolved(None).unwrap(), WORLDS_CONTENT_SERVER);
    }

    /// A path in the value is what makes it verbatim now that the shape rule
    /// covers the env var too, so this stays network-free.
    #[tokio::test]
    async fn the_env_default_outranks_the_worlds_default() {
        let _guard = ENV_LOCK.lock().await;
        let target = resolved(
            Some("http://127.0.0.1:9/content"),
            Some("gather.dcl.eth"),
            true,
        )
        .await;
        assert_eq!(target.unwrap(), "http://127.0.0.1:9/content");
    }

    #[tokio::test]
    async fn an_explicit_target_content_outranks_the_worlds_default_and_the_env() {
        let _guard = ENV_LOCK.lock().await;
        set_env(Some("http://127.0.0.1:9"));
        let target = resolve_target_from(
            None,
            Some("https://example.org/"),
            Some("gather.dcl.eth"),
            true,
            TargetConsent::default(),
        )
        .await;
        set_env(None);
        assert_eq!(target.unwrap(), "https://example.org");
    }

    /// The exact shape `unpublish.rs` resolves with: land-only, so the worlds
    /// default must not leak in.
    #[tokio::test]
    async fn land_unpublish_resolution_still_refuses_without_a_target() {
        let _guard = ENV_LOCK.lock().await;
        let out = resolved(None, None, true).await;
        let err = format!("{:#}", out.expect_err("land + key must still refuse"));
        assert!(err.contains("no deploy target given"), "{err}");
    }
}
