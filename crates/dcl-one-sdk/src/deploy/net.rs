use super::{
    catalyst_rotation, configured_catalyst_rotation, human_size, now_ms, DeployOptions,
    UPSTREAM_CATALYST_HOSTS,
};
use crate::ux::{self, TrySteps, UserError};
use anyhow::{bail, Context, Result};
use catalyrst_crypto::Wallet;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::HashSet;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
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

/// How far an upload has got, shared between the carrier sending it and
/// whoever draws it: the signing page polls this while the wallet's
/// signature travels, so a 20 MB scene is not a bare "Uploading…" for a
/// minute. Cheap to clone; every clone reads the same state.
#[derive(Clone, Default)]
pub struct UploadProgress(Arc<Mutex<ProgressState>>);

/// One snapshot of an upload. `total` and `sent` count body bytes on the
/// wire (the multipart framing included, which is why `total` can exceed the
/// payload size by a few KB); `files_sent` is how many payload files are
/// fully sent and `current` the one in flight.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ProgressState {
    /// idle · staging · uploading · validating · done · failed. `validating`
    /// is the stretch after the last byte left and before the server
    /// answered: the content server checking the deployment.
    pub phase: &'static str,
    /// node · curl · reqwest. curl carries no byte counts, and the page says
    /// so instead of drawing a bar that never moves.
    pub carrier: &'static str,
    pub total: u64,
    pub sent: u64,
    pub files: usize,
    pub files_sent: usize,
    pub current: Option<String>,
    pub started_ms: i64,
    pub sent_ms: i64,
    /// [`Reuse::sentence`] once something stayed home: why `files` can read
    /// smaller than the payload row above the bar.
    pub reuse: Option<String>,
}

impl Default for ProgressState {
    fn default() -> Self {
        ProgressState {
            phase: "idle",
            carrier: "",
            total: 0,
            sent: 0,
            files: 0,
            files_sent: 0,
            current: None,
            started_ms: 0,
            sent_ms: 0,
            reuse: None,
        }
    }
}

fn payload_len(entity_len: usize, files: &Files) -> u64 {
    entity_len as u64 + files.iter().map(|(_, _, b)| b.len() as u64).sum::<u64>()
}

impl UploadProgress {
    pub fn snapshot(&self) -> ProgressState {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn update(&self, f: impl FnOnce(&mut ProgressState)) {
        f(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner));
    }

    /// The look at what the server holds, before anything travels: the
    /// whole payload is the size on show until [`Self::begin`] narrows it.
    fn checking(&self, entity_len: usize, files: &Files) {
        self.update(|p| {
            *p = ProgressState {
                phase: "checking",
                total: payload_len(entity_len, files),
                files: files.len(),
                started_ms: now_ms(),
                ..ProgressState::default()
            }
        });
    }

    /// `files` is what travels; `reuse` says what stayed home.
    fn begin(&self, entity_len: usize, files: &Files, reuse: Reuse) {
        self.update(|p| {
            *p = ProgressState {
                phase: "staging",
                total: payload_len(entity_len, files),
                files: files.len(),
                reuse: (reuse.reused_files > 0).then(|| reuse.sentence()),
                started_ms: now_ms(),
                ..ProgressState::default()
            }
        });
    }

    fn carrier(&self, name: &'static str) {
        self.update(|p| {
            p.carrier = name;
            p.phase = "uploading";
        });
    }

    /// A carrier's report: bytes on the wire so far, the wire total, and the
    /// payload file in flight (`None` before the first file; past the end
    /// once every file is out). The last byte leaving flips the phase to
    /// `validating`, since from then on the wait is the server's.
    fn note(&self, sent: u64, total: u64, file: Option<usize>, names: &[String]) {
        self.update(|p| {
            if total > 0 {
                p.total = total;
            }
            p.sent = sent.min(p.total.max(sent));
            match file {
                Some(i) if i < names.len() => {
                    p.files_sent = i;
                    p.current = Some(names[i].clone());
                }
                Some(_) => {
                    p.files_sent = names.len();
                    p.current = None;
                }
                None => {
                    p.files_sent = 0;
                    p.current = None;
                }
            }
            if p.total > 0 && p.sent >= p.total {
                if p.phase != "validating" {
                    p.sent_ms = now_ms();
                }
                p.phase = "validating";
                p.files_sent = names.len();
                p.current = None;
            } else {
                p.phase = "uploading";
            }
        });
    }

    fn finish(&self, ok: bool) {
        self.update(|p| {
            if ok {
                p.sent = p.total;
                p.files_sent = p.files;
                p.current = None;
            }
            p.phase = if ok { "done" } else { "failed" };
        });
    }
}

/// The multipart body every carrier sends, part by part and in the order
/// the server reads it: the text fields, the entity, then each payload file.
/// Each part carries the index of the payload file it belongs to (`None`
/// before the first file, `files.len()` for the closing boundary), which is
/// what a counted send reports as "the file in flight".
fn multipart_parts(
    boundary: &str,
    entity_id: &str,
    entity_bytes: &[u8],
    files: &Files,
    auth_chain: &Value,
) -> Vec<(Option<usize>, Vec<u8>)> {
    fn text(parts: &mut Vec<(Option<usize>, Vec<u8>)>, boundary: &str, name: &str, value: &str) {
        parts.push((
            None,
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .into_bytes(),
        ));
    }
    fn blob(
        parts: &mut Vec<(Option<usize>, Vec<u8>)>,
        boundary: &str,
        file: Option<usize>,
        name: &str,
        mime: &str,
        bytes: &[u8],
    ) {
        parts.push((
            file,
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{name}\"\r\nContent-Type: {mime}\r\n\r\n"
            )
            .into_bytes(),
        ));
        parts.push((file, bytes.to_vec()));
        parts.push((file, b"\r\n".to_vec()));
    }
    let mut parts = Vec::new();
    text(&mut parts, boundary, "entityId", entity_id);
    text(
        &mut parts,
        boundary,
        "authChain",
        &serde_json::to_string(auth_chain).unwrap_or_default(),
    );
    for (i, k, v) in chain_fields(auth_chain) {
        text(&mut parts, boundary, &format!("authChain[{i}][{k}]"), v);
    }
    blob(
        &mut parts,
        boundary,
        None,
        entity_id,
        "application/json",
        entity_bytes,
    );
    for (i, (_, hash, bytes)) in files.iter().enumerate() {
        blob(
            &mut parts,
            boundary,
            Some(i),
            hash,
            "application/octet-stream",
            bytes,
        );
    }
    parts.push((
        Some(files.len()),
        format!("--{boundary}--\r\n").into_bytes(),
    ));
    parts
}

/// The body as a stream of 64 KB chunks that reports each one to `progress`
/// as the client pulls it — which it does as the socket drains, so the
/// count tracks the wire within a buffer or two.
fn counted_body(
    parts: Vec<(Option<usize>, Vec<u8>)>,
    progress: UploadProgress,
    names: Arc<Vec<String>>,
) -> reqwest::Body {
    reqwest::Body::wrap_stream(counted_stream(parts, progress, names))
}

fn counted_stream(
    parts: Vec<(Option<usize>, Vec<u8>)>,
    progress: UploadProgress,
    names: Arc<Vec<String>>,
) -> impl futures::Stream<Item = Result<Vec<u8>, std::io::Error>> + Send + 'static {
    const CHUNK: usize = 64 * 1024;
    let total: u64 = parts.iter().map(|(_, b)| b.len() as u64).sum();
    futures::stream::unfold(
        (parts, 0usize, 0usize, 0u64),
        move |(parts, pi, off, sent)| {
            let progress = progress.clone();
            let names = names.clone();
            async move {
                if pi >= parts.len() {
                    return None;
                }
                let (file, bytes) = &parts[pi];
                let end = (off + CHUNK).min(bytes.len());
                let chunk = bytes[off..end].to_vec();
                let sent = sent + (end - off) as u64;
                progress.note(sent, total, *file, &names);
                let (pi, off) = if end >= bytes.len() {
                    (pi + 1, 0)
                } else {
                    (pi, end)
                };
                Some((Ok::<_, std::io::Error>(chunk), (parts, pi, off, sent)))
            }
        },
    )
}

fn file_names(files: &Files) -> Arc<Vec<String>> {
    Arc::new(files.iter().map(|(path, _, _)| path.clone()).collect())
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
    progress: &UploadProgress,
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
    let child = tokio::process::Command::new(&node)
        .arg(dir.join("up.mjs"))
        .arg(dir.join("cfg.json"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Some(Err(anyhow::anyhow!("node could not run: {e}")));
        }
    };
    progress.carrier("node");
    // The script reports the send on stderr, one JSON line per chunk batch;
    // anything else there is its failure message. stdout is the answer.
    let reporter = {
        let stderr = child.stderr.take();
        let progress = progress.clone();
        let names = file_names(files);
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let Some(stderr) = stderr else {
                return;
            };
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some((sent, total, file)) = parse_progress_line(&line) {
                    progress.note(sent, total, file, &names);
                } else if !line.trim().is_empty() {
                    tracing::debug!("node upload: {line}");
                }
            }
        })
    };
    let out = child.wait_with_output().await;
    let _ = reporter.await;
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
const read = (name) => readFileSync(join(cfg.dir, name));
const enc = new TextEncoder();
const boundary = '----dclonesdk' + Math.random().toString(16).slice(2) + Date.now().toString(16);
// The body, part by part, in the order the server reads it: the text fields,
// the entity, then every payload file. `file` is the index into cfg.files a
// part belongs to (null before the first file, cfg.files.length for the
// closing boundary): what the progress lines name as the file in flight.
const parts = [];
const text = (name, value) =>
  parts.push({ file: null, bytes: enc.encode(`--${boundary}\r\nContent-Disposition: form-data; name="${name}"\r\n\r\n${value}\r\n`) });
const blob = (file, name, type) => {
  parts.push({ file, bytes: enc.encode(`--${boundary}\r\nContent-Disposition: form-data; name="${name}"; filename="${name}"\r\nContent-Type: ${type}\r\n\r\n`) });
  parts.push({ file, bytes: read(name) });
  parts.push({ file, bytes: enc.encode('\r\n') });
};
text('entityId', cfg.entityId);
text('authChain', JSON.stringify(cfg.authChain));
cfg.authChain.forEach((l, i) => {
  text(`authChain[${i}][type]`, l.type);
  text(`authChain[${i}][payload]`, l.payload);
  text(`authChain[${i}][signature]`, l.signature);
});
blob(null, cfg.entityId, 'application/json');
cfg.files.forEach((hash, i) => blob(i, hash, 'application/octet-stream'));
parts.push({ file: cfg.files.length, bytes: enc.encode(`--${boundary}--\r\n`) });
const total = parts.reduce((n, p) => n + p.bytes.length, 0);
// Progress goes to stderr as JSON lines, at most every 150ms or 512KB, and
// always for the last byte. fetch pulls a chunk as the socket drains, so the
// count tracks the wire within a buffer or two.
const CHUNK = 64 * 1024;
let sent = 0, reportedAt = 0, reportedSent = 0;
const report = (file, force) => {
  const now = Date.now();
  if (!force && now - reportedAt < 150 && sent - reportedSent < 512 * 1024) return;
  reportedAt = now; reportedSent = sent;
  process.stderr.write(JSON.stringify({ sent, total, file }) + '\n');
};
let pi = 0, off = 0;
const body = new ReadableStream({
  pull(controller) {
    if (pi >= parts.length) { report(cfg.files.length, true); controller.close(); return; }
    const p = parts[pi];
    const end = Math.min(off + CHUNK, p.bytes.length);
    controller.enqueue(p.bytes.subarray(off, end));
    sent += end - off; off = end;
    if (off >= p.bytes.length) { pi += 1; off = 0; }
    report(p.file, false);
  },
});
const send = async (body, extra) => {
  const r = await fetch(cfg.url, {
    method: 'POST',
    headers: { 'content-type': `multipart/form-data; boundary=${boundary}` },
    body,
    ...extra,
  });
  return { status: r.status, body: await r.text() };
};
try {
  let out;
  try {
    out = await send(body, { duplex: 'half' });
  } catch (e) {
    // A Node without streaming request bodies refuses the stream before
    // connecting: send the same bytes in one piece, without progress.
    if (!/duplex/i.test(String(e && e.message))) throw e;
    const whole = new Uint8Array(total);
    let o = 0;
    for (const p of parts) { whole.set(p.bytes, o); o += p.bytes.length; }
    out = await send(whole, {});
  }
  process.stdout.write(JSON.stringify(out));
} catch (e) {
  process.stderr.write(String(e && e.message ? e.message : e));
  process.exit(1);
}
"#;

/// One of the node script's stderr lines, when it is a progress report.
fn parse_progress_line(line: &str) -> Option<(u64, u64, Option<usize>)> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    let sent = v.get("sent")?.as_u64()?;
    let total = v.get("total")?.as_u64()?;
    let file = v.get("file").and_then(Value::as_u64).map(|f| f as usize);
    Some((sent, total, file))
}

/// Upload with curl, the fallback when Node is absent: its multipart
/// fingerprint passes edges that challenge reqwest's. `None` means curl is
/// absent too.
async fn curl_upload(
    url: &str,
    entity_id: &str,
    entity_bytes: &[u8],
    files: &Files,
    auth_chain: &Value,
    progress: &UploadProgress,
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
    // curl's meter is not machine-readable; the page shows the payload size
    // and the clock instead of a bar that never moves.
    progress.carrier("curl");
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
    progress: &UploadProgress,
) -> Result<(u16, String)> {
    let boundary = format!("----dclonesdk{:x}{:x}", rand::random::<u64>(), now_ms());
    let parts = multipart_parts(&boundary, entity_id, &entity_bytes, files, auth_chain);
    progress.carrier("reqwest");
    let body = counted_body(parts, progress.clone(), file_names(files));
    send_text(
        upload_client()?
            .post(url)
            .header(
                reqwest::header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body),
    )
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
        (Some(t), None) => target_content_url(t, "--target-server", world.is_some()).await?,
        (None, None) => match (configured_target_server(), world) {
            (Some(t), _) => {
                target_content_url(&t, "DCL_ONE_SDK_TARGET_SERVER", world.is_some()).await?
            }
            (None, Some(w)) => {
                if !consent.quiet {
                    ux::note(format!(
                        "deploying the world \"{w}\" to the public worlds server {WORLDS_CONTENT_SERVER}"
                    ));
                }
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
/// spells out where the entities route lives skips the probe. A worlds
/// server is verbatim regardless of path — the public one by name, and any
/// host a world scene is sent to, since a worlds server answers /status,
/// never a catalyst's /about, and a /about probe against a Cloudflare edge
/// gets the following upload challenged (that probe was the reason two
/// flags once existed).
async fn target_content_url(t: &str, source: &str, world_scene: bool) -> Result<String> {
    let t = t.trim();
    let base = sanitize_catalyst_url(t);
    let worlds = world_scene || host_of(&base) == host_of(WORLDS_CONTENT_SERVER);
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
        &UploadProgress::default(),
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

/// Cids per `available-content` question: eighty keep the URL under the
/// shortest limit an edge enforces.
const STORED_BATCH: usize = 80;

/// The split every surface states — the /target forecast, the /deploy head,
/// the terminal and the signing panel's progress: files the server already
/// holds against the ones that travel, by count and bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reuse {
    pub reused_files: usize,
    pub reused_bytes: u64,
    pub upload_files: usize,
    pub upload_bytes: u64,
}

impl Reuse {
    /// Tallies `(held by the server, size)` per file.
    pub fn tally(files: impl IntoIterator<Item = (bool, u64)>) -> Reuse {
        let mut r = Reuse::default();
        for (held, bytes) in files {
            let (count, sum) = match held {
                true => (&mut r.reused_files, &mut r.reused_bytes),
                false => (&mut r.upload_files, &mut r.upload_bytes),
            };
            *count += 1;
            *sum += bytes;
        }
        r
    }

    /// The one sentence about the split, the same wherever it is said.
    pub fn sentence(&self) -> String {
        let s = |n: usize| if n == 1 { "" } else { "s" };
        match (self.reused_files, self.upload_files) {
            (0, up) => format!(
                "All {up} file{} upload ({}) — the server has none of them yet",
                s(up),
                human_size(self.upload_bytes)
            ),
            (kept, 0) => format!(
                "All {kept} file{} are already on the server ({}), republishing only updates the deployment timestamp",
                s(kept),
                human_size(self.reused_bytes)
            ),
            (kept, up) => format!(
                "{kept} of {} files are already on the server — {up} to upload ({})",
                kept + up,
                human_size(self.upload_bytes)
            ),
        }
    }
}

/// Which of `cids` the content server at `base` already stores, by
/// `GET /available-content?cid=…`: the /target forecast's question and,
/// before the upload, the reason stored files stay home. Every content
/// server answers it and accepts an entity whose stored files are not in
/// the request — the skip the upstream toolchain makes — so a republish
/// after a one-texture edit sends the entity and that texture.
///
/// The question travels the way the upload does (node, then curl, then
/// reqwest): a Cloudflare-fronted worlds server challenges the reqwest
/// fingerprint and then the upload from the IP it just challenged, so the
/// look-ahead has to be what the edge already lets through. Whatever goes
/// unanswered counts as not held — a missed skip costs bandwidth, a wrong
/// one the deploy. `DCL_ONE_SDK_UPLOAD_ALL=1` sends everything regardless.
pub(crate) async fn stored_cids(base: &str, cids: &[&str]) -> HashSet<String> {
    if upload_all_from(std::env::var_os("DCL_ONE_SDK_UPLOAD_ALL")) {
        return HashSet::new();
    }
    let mut cids = cids.to_vec();
    cids.sort_unstable();
    cids.dedup();
    if cids.is_empty() {
        return HashSet::new();
    }
    let base = base.trim_end_matches('/');
    let urls: Vec<String> = cids
        .chunks(STORED_BATCH)
        .map(|batch| {
            let query: Vec<String> = batch.iter().map(|c| format!("cid={c}")).collect();
            format!("{base}/available-content?{}", query.join("&"))
        })
        .collect();
    let mut have = HashSet::new();
    for body in fetch_all(&urls).await.iter().flatten() {
        have.extend(parse_available(body));
    }
    have
}

fn upload_all_from(raw: Option<std::ffi::OsString>) -> bool {
    raw.is_some_and(|v| !v.is_empty() && v != "0")
}

/// The cids an `available-content` answer marks available; a body that is
/// not that answer names none.
fn parse_available(body: &str) -> Vec<String> {
    serde_json::from_str::<Vec<Value>>(body)
        .unwrap_or_default()
        .iter()
        .filter(|e| e.get("available").and_then(Value::as_bool) == Some(true))
        .filter_map(|e| e.get("cid").and_then(Value::as_str).map(str::to_string))
        .collect()
}

/// The files that travel, and the split: a file whose hash the server
/// holds is left out of the request. Borrowed when nothing stays, so the
/// common case copies no bytes.
pub(crate) fn split_stored<'a>(
    files: &'a Files,
    have: &HashSet<String>,
) -> (Cow<'a, Files>, Reuse) {
    let reuse = Reuse::tally(
        files
            .iter()
            .map(|(_, h, b)| (have.contains(h), b.len() as u64)),
    );
    if reuse.reused_files == 0 {
        return (Cow::Borrowed(files), reuse);
    }
    let send: Vec<(String, String, Vec<u8>)> = files
        .iter()
        .filter(|(_, hash, _)| !have.contains(hash))
        .cloned()
        .collect();
    (Cow::Owned(send), reuse)
}

/// One GET per url, all by one carrier — node, then curl, then reqwest, the
/// upload's order — with `None` where a url went unanswered or answered
/// outside 2xx. As with the upload, a carrier that is present is the one
/// that answers: a node that fails does not hand the question to curl.
async fn fetch_all(urls: &[String]) -> Vec<Option<String>> {
    if let Some(v) = node_fetch_all(urls).await {
        return v;
    }
    if let Some(v) = curl_fetch_all(urls).await {
        return v;
    }
    reqwest_fetch_all(urls).await
}

fn body_when_ok(status: Option<u64>, body: Option<&str>) -> Option<String> {
    match (status, body) {
        (Some(s), Some(b)) if (200..300).contains(&s) => Some(b.to_string()),
        _ => None,
    }
}

/// The urls come as arguments; the answers leave as one JSON array on
/// stdout, in order, status 0 for a url that never answered. Six in
/// flight at once, so a payload of thousands of files does not open a
/// connection per eighty of them all at the same moment.
const NODE_FETCH_JS: &str = r#"const urls = process.argv.slice(1);
const one = (u) => fetch(u, { signal: AbortSignal.timeout(10000) })
  .then(async (r) => ({ status: r.status, body: await r.text() }))
  .catch(() => ({ status: 0, body: '' }));
const out = new Array(urls.length);
let next = 0;
const worker = async () => { while (next < urls.length) { const i = next++; out[i] = await one(urls[i]); } };
Promise.all(Array.from({ length: Math.min(6, urls.length) }, worker))
  .then(() => process.stdout.write(JSON.stringify(out)));"#;

async fn node_fetch_all(urls: &[String]) -> Option<Vec<Option<String>>> {
    let node = crate::build::find_node()?;
    let unanswered = || Some(vec![None; urls.len()]);
    let out = match tokio::process::Command::new(&node)
        .arg("-e")
        .arg(NODE_FETCH_JS)
        .args(urls)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .await
    {
        Ok(o) => o,
        Err(_) => return unanswered(),
    };
    let Ok(answers) = serde_json::from_slice::<Vec<Value>>(&out.stdout) else {
        return unanswered();
    };
    if answers.len() != urls.len() {
        return unanswered();
    }
    Some(
        answers
            .iter()
            .map(|a| {
                body_when_ok(
                    a.get("status").and_then(Value::as_u64),
                    a.get("body").and_then(Value::as_str),
                )
            })
            .collect(),
    )
}

async fn curl_fetch_all(urls: &[String]) -> Option<Vec<Option<String>>> {
    let mut out = Vec::with_capacity(urls.len());
    for url in urls {
        let run = tokio::process::Command::new("curl")
            .args([
                "-sS",
                "-A",
                USER_AGENT,
                "--max-time",
                "10",
                "-w",
                "\n%{http_code}",
            ])
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .await;
        match run {
            Ok(o) => {
                let text = String::from_utf8_lossy(&o.stdout);
                let (body, code) = text.rsplit_once('\n').unwrap_or(("", "0"));
                out.push(body_when_ok(code.trim().parse().ok(), Some(body)));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(_) => out.push(None),
        }
    }
    Some(out)
}

async fn reqwest_fetch_all(urls: &[String]) -> Vec<Option<String>> {
    let Ok(client) = probe_client() else {
        return vec![None; urls.len()];
    };
    futures::future::join_all(urls.iter().map(|u| {
        let client = client.clone();
        async move {
            match send_text(client.get(u)).await {
                Ok((code, body)) => body_when_ok(Some(code as u64), Some(&body)),
                Err(_) => None,
            }
        }
    }))
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn upload_entity_to(
    target: &str,
    entity_id: &str,
    entity_bytes: Vec<u8>,
    files: &Files,
    address: &str,
    signature: &str,
    destination: UploadDestination<'_>,
    progress: &UploadProgress,
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
        progress,
    )
    .await
}

/// `progress` is where the send reports itself; a caller with nothing to
/// draw passes a fresh [`UploadProgress`] and never reads it.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn upload_entity_with_chain_to(
    target: &str,
    entity_id: &str,
    entity_bytes: Vec<u8>,
    files: &Files,
    address: &str,
    auth_chain: Value,
    destination: UploadDestination<'_>,
    progress: &UploadProgress,
) -> Result<String> {
    progress.checking(entity_bytes.len(), files);
    let url = format!("{}/entities", target.trim_end_matches('/'));
    tracing::info!("uploading to {url} as {address} (entity {entity_id})");
    // Keep a publish legible beside watch events: the action owns the clock,
    // while the long URL and signer get their own continuation lines.
    ux::note_clocked(destination.headline());
    ux::note_arrow(format!("url: {url}"));
    ux::note_arrow(format!("signer: {address}"));

    // What the server already holds stays home; the entity always travels.
    let cids: Vec<&str> = files.iter().map(|(_, h, _)| h.as_str()).collect();
    let (send, reuse) = split_stored(files, &stored_cids(target, &cids).await);
    let files: &Files = &send;
    if reuse.reused_files > 0 {
        ux::note_arrow(reuse.sentence());
    }
    progress.begin(entity_bytes.len(), files, reuse);

    // Node carries the upload, then curl, then reqwest. A Cloudflare-fronted
    // worlds server challenges reqwest and curl but not Node — the official
    // tooling is Node, so its fingerprint is the one the edge accepts. This
    // is only reliable because the one request the deploy makes to the
    // content server before it, the look at what it holds, travels by the
    // same carrier: a reqwest pre-flight would flag the IP and the upload
    // that follows would inherit the challenge.
    let carried = match node_upload(&url, entity_id, &entity_bytes, files, &auth_chain, progress)
        .await
    {
        Some(r) => r,
        None => {
            match curl_upload(&url, entity_id, &entity_bytes, files, &auth_chain, progress).await {
                Some(r) => r,
                None => {
                    reqwest_upload(&url, entity_id, entity_bytes, files, &auth_chain, progress)
                        .await
                }
            }
        }
    };
    let (status, body) = match carried {
        Ok(x) => x,
        Err(e) => {
            progress.finish(false);
            return Err(e);
        }
    };

    if status == 0 {
        // curl reached no server (connection refused, DNS failure, timeout):
        // no HTTP response, so `-w %{http_code}` prints 000. Same sentence
        // the reqwest transport error gives.
        progress.finish(false);
        return Err(cannot_reach(format!("no response from {url}")).into());
    }
    if (200..300).contains(&status) {
        tracing::info!("deployed \u{2713} (HTTP {status}) — server: {body}");
        progress.finish(true);
        Ok(format!(
            "Deployed {entity_id} to {} (HTTP {status})",
            host_of(&url).unwrap_or_default()
        ))
    } else {
        progress.finish(false);
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
        assert!(e.contains("\"Select LAND\""), "and the other way out: {e}");
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
        // A world sent to a self-hosted worlds server: verbatim too. Port 9
        // answers nothing, so a /about probe would have failed this.
        let own_worlds = resolved(Some("127.0.0.1:9"), Some("w.dcl.eth"), true).await;
        assert_eq!(
            own_worlds.unwrap(),
            "https://127.0.0.1:9",
            "a world scene's target is a worlds server, no /about probe"
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

    /// The body a counted send streams is the multipart every carrier
    /// sends, and the count it reports walks the files in order and lands on
    /// "validating" with the last byte.
    #[tokio::test]
    async fn a_counted_send_reports_the_file_in_flight_then_validating() {
        use futures::StreamExt;
        let files: Vec<(String, String, Vec<u8>)> = vec![
            ("a.bin".into(), "bafya".into(), vec![1u8; 70_000]),
            ("b.bin".into(), "bafyb".into(), vec![2u8; 10]),
        ];
        let chain = json!([{ "type": "SIGNER", "payload": "0xabc", "signature": "" }]);
        let parts = multipart_parts("XYZ", "bafyentity", b"{}", &files, &chain);
        let wire: Vec<u8> = parts.iter().flat_map(|(_, b)| b.clone()).collect();
        let text = String::from_utf8_lossy(&wire);
        assert!(text.starts_with(
            "--XYZ\r\nContent-Disposition: form-data; name=\"entityId\"\r\n\r\nbafyentity\r\n"
        ));
        assert!(text.contains("name=\"authChain[0][type]\"\r\n\r\nSIGNER\r\n"));
        assert!(text.contains("name=\"bafyentity\"; filename=\"bafyentity\"\r\nContent-Type: application/json\r\n\r\n{}\r\n"));
        assert!(text.contains(
            "name=\"bafya\"; filename=\"bafya\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        ));
        assert!(text.ends_with("--XYZ--\r\n"));
        let total = wire.len() as u64;

        let progress = UploadProgress::default();
        progress.begin(2, &files, Reuse::default());
        assert_eq!(progress.snapshot().phase, "staging");
        assert_eq!(progress.snapshot().total, 70_012);
        assert_eq!(progress.snapshot().files, 2);
        let names = file_names(&files);
        let mut seen = Vec::new();
        let mut body = Box::pin(counted_stream(parts, progress.clone(), names));
        while let Some(chunk) = body.next().await {
            let n = chunk.unwrap().len();
            seen.push((n, progress.snapshot()));
        }
        let streamed: usize = seen.iter().map(|(n, _)| n).sum();
        assert_eq!(streamed as u64, total, "every byte of the body is streamed");
        let during: Vec<_> = seen
            .iter()
            .filter(|(_, p)| p.phase == "uploading")
            .map(|(_, p)| (p.files_sent, p.current.clone()))
            .collect();
        assert!(
            during.contains(&(0, Some("a.bin".into()))),
            "the first file is named while it goes out: {during:?}"
        );
        assert!(during.contains(&(1, Some("b.bin".into()))), "{during:?}");
        let last = &seen.last().unwrap().1;
        assert_eq!(last.phase, "validating", "{last:?}");
        assert_eq!(
            (last.sent, last.total, last.files_sent, &last.current),
            (total, total, 2, &None)
        );
        assert!(last.sent_ms >= last.started_ms);
        progress.finish(false);
        assert_eq!(progress.snapshot().phase, "failed");
    }

    /// What the node script writes on stderr: progress lines are consumed,
    /// anything else is not mistaken for one.
    #[test]
    fn node_progress_lines_parse_and_prose_does_not() {
        assert_eq!(
            parse_progress_line(r#"{"sent":1024,"total":4096,"file":2}"#),
            Some((1024, 4096, Some(2)))
        );
        assert_eq!(
            parse_progress_line(r#"{"sent":10,"total":4096,"file":null}"#),
            Some((10, 4096, None))
        );
        assert_eq!(parse_progress_line("fetch failed"), None);
        assert_eq!(parse_progress_line(r#"{"status":200}"#), None);
    }

    async fn serve(app: axum::Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        base
    }

    /// The look-ahead: a hundred cids travel as two questions, the files
    /// the server holds stay home, the entity and the rest go, the
    /// multipart never names a file that stayed, and the progress carries
    /// the sentence about it.
    #[tokio::test]
    async fn files_the_server_already_holds_stay_home() {
        use axum::extract::Query;
        use axum::routing::get;
        use axum::{Json, Router};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let app = Router::new().route(
            "/available-content",
            get(move |Query(q): Query<Vec<(String, String)>>| {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let answer: Vec<Value> = q
                        .iter()
                        .filter(|(k, _)| k == "cid")
                        .map(|(_, c)| json!({ "cid": c, "available": c.ends_with('0') }))
                        .collect();
                    Json(answer)
                }
            }),
        );
        let base = serve(app).await;
        let files: Vec<(String, String, Vec<u8>)> = (0..100u8)
            .map(|i| {
                (
                    format!("f{i}.bin"),
                    format!("bafy{i}"),
                    vec![i; 10 + i as usize],
                )
            })
            .collect();
        let cids: Vec<&str> = files.iter().map(|(_, h, _)| h.as_str()).collect();
        let have = stored_cids(&format!("{base}/"), &cids).await;
        assert_eq!(hits.load(Ordering::SeqCst), 2, "two batches of eighty");
        let (send, reuse) = split_stored(&files, &have);
        assert_eq!(
            (reuse.reused_files, reuse.upload_files),
            (10, 90),
            "{have:?}"
        );
        assert_eq!(
            reuse.reused_bytes,
            (0..100u64).step_by(10).map(|i| 10 + i).sum::<u64>()
        );
        assert_eq!(reuse.upload_bytes, payload_len(0, &send));
        assert!(matches!(send, Cow::Owned(_)));
        assert!(send.iter().all(|(_, h, _)| !h.ends_with('0')));
        let chain = json!([{ "type": "SIGNER", "payload": "0xabc", "signature": "" }]);
        let parts = multipart_parts("XYZ", "bafyentity", b"{}", &send, &chain);
        let wire: Vec<u8> = parts.iter().flat_map(|(_, b)| b.clone()).collect();
        let text = String::from_utf8_lossy(&wire);
        assert!(
            text.contains("name=\"bafyentity\"; filename=\"bafyentity\""),
            "the entity always travels"
        );
        assert!(text.contains("name=\"bafy11\"; filename=\"bafy11\""));
        assert!(
            !text.contains("name=\"bafy10\""),
            "a stored file never travels"
        );
        let progress = UploadProgress::default();
        progress.checking(2, &files);
        assert_eq!(
            (progress.snapshot().phase, progress.snapshot().files),
            ("checking", 100)
        );
        progress.begin(2, &send, reuse);
        let p = progress.snapshot();
        assert_eq!((p.phase, p.files), ("staging", 90));
        assert_eq!(p.reuse.as_deref(), Some(reuse.sentence().as_str()));
        assert_eq!(p.total, 2 + payload_len(0, &send));
    }

    /// A server that cannot answer, or answers outside 2xx, keeps nothing
    /// home: everything travels, the common case copies no bytes, and the
    /// progress has nothing to say about it.
    #[tokio::test]
    async fn an_unanswered_look_ahead_sends_everything() {
        use axum::routing::get;
        use axum::Router;
        let app = Router::new().route(
            "/available-content",
            get(|| async { (axum::http::StatusCode::BAD_GATEWAY, "edge") }),
        );
        let base = serve(app).await;
        assert!(stored_cids(&base, &["bafya"]).await.is_empty());
        assert!(
            stored_cids("http://127.0.0.1:9", &["bafya"])
                .await
                .is_empty(),
            "a closed port"
        );
        let files: Vec<(String, String, Vec<u8>)> =
            vec![("a.bin".into(), "bafya".into(), vec![1u8; 3])];
        let (send, reuse) = split_stored(&files, &HashSet::new());
        assert_eq!(
            (reuse.reused_files, reuse.upload_files, reuse.upload_bytes),
            (0, 1, 3)
        );
        assert!(matches!(send, Cow::Borrowed(_)));
        let progress = UploadProgress::default();
        progress.begin(2, &send, reuse);
        assert_eq!(progress.snapshot().reuse, None, "nothing stayed home");
    }

    /// The one sentence every surface says, the answer parser, and the
    /// switch that sends everything.
    #[test]
    fn the_split_sentence_the_answer_and_the_upload_all_switch() {
        let tally = |held: &[bool]| Reuse::tally(held.iter().map(|h| (*h, 2_500_000)));
        assert_eq!(
            tally(&[true, true, true, true, false]).sentence(),
            "4 of 5 files are already on the server — 1 to upload (2.5 MB)"
        );
        assert_eq!(
            tally(&[true; 10]).sentence(),
            "All 10 files are already on the server (25.0 MB), republishing only updates the deployment timestamp"
        );
        assert_eq!(
            tally(&[false]).sentence(),
            "All 1 file upload (2.5 MB) — the server has none of them yet"
        );
        assert_eq!(
            parse_available(
                r#"[{"cid":"a","available":true},{"cid":"b","available":false},{"cid":"c"}]"#
            ),
            vec!["a".to_string()]
        );
        assert!(parse_available("<html>challenge</html>").is_empty());
        assert!(parse_available(r#"{"cid":"a","available":true}"#).is_empty());
        assert!(!upload_all_from(None));
        assert!(!upload_all_from(Some("".into())));
        assert!(!upload_all_from(Some("0".into())));
        assert!(upload_all_from(Some("1".into())));
        assert_eq!(body_when_ok(Some(200), Some("x")).as_deref(), Some("x"));
        assert_eq!(body_when_ok(Some(403), Some("x")), None);
        assert_eq!(body_when_ok(None, Some("x")), None);
    }
}
