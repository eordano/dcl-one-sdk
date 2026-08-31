use crate::deploy::{self, Prepared};
use crate::ux::{self, TrySteps, UserError};
use anyhow::Result;
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

pub struct LinkerDeploy {
    /// The scene root, so the browser flow can serve the scene it is about to
    /// publish: the signing page is the preview server's landing page.
    pub dir: PathBuf,
    pub prepared: Prepared,
    pub target_content: String,
    pub world: Option<String>,
    pub needs_delete: bool,
    pub timestamp_override: Option<i64>,
    pub entity_out: Option<PathBuf>,
    pub scene_title: String,
    pub base_parcel: String,
    pub multi_scene: bool,
    pub check_permissions: bool,
}

pub struct LinkerOptions {
    pub port: Option<u16>,
    pub open_browser: bool,
    pub timeout: Duration,
    /// When set, the caller hosts the signing routes on a server it already
    /// runs (the preview server mounts them under /deploy/sign/): no listener
    /// is bound here and no browser is opened — the page owns the hand-off.
    pub host: Option<HostSigner>,
}

/// How a hosting caller receives the signing state, and the URL people see.
#[derive(Clone)]
pub struct HostSigner {
    pub register: Arc<dyn Fn(Arc<LinkerState>) + Send + Sync>,
    pub url: String,
}

pub const DEFAULT_TIMEOUT_SECS: u64 = 600;

pub fn linker_timeout() -> Duration {
    let secs = std::env::var("DCL_ONE_SDK_LINKER_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

struct PendingEntity {
    bytes: Vec<u8>,
    delete_payload: Option<String>,
}

type DoneSender = tokio::sync::oneshot::Sender<Result<String>>;

pub struct LinkerState {
    dep: LinkerDeploy,
    pending: Mutex<HashMap<String, PendingEntity>>,
    done: Mutex<Option<DoneSender>>,
    /// The address that signed, kept past the upload: the preview pages
    /// personalize on "the wallet you sign with", and a signature is the one
    /// moment that wallet names itself.
    signer: Mutex<Option<String>>,
}

impl LinkerState {
    pub(crate) fn target_content(&self) -> &str {
        &self.dep.target_content
    }

    pub(crate) fn signer_address(&self) -> Option<String> {
        self.signer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// What [`sign`] does the moment a signature arrives, callable by tests
    /// that need a "the wallet already answered" state without a wallet.
    #[cfg(test)]
    pub(crate) fn note_signer_for_tests(&self, address: &str) {
        *self.signer.lock().unwrap_or_else(PoisonError::into_inner) = Some(address.to_string());
    }
}

pub fn new_state(
    dep: LinkerDeploy,
) -> (
    Arc<LinkerState>,
    tokio::sync::oneshot::Receiver<Result<String>>,
) {
    let (tx, rx) = tokio::sync::oneshot::channel();
    (
        Arc::new(LinkerState {
            dep,
            pending: Mutex::new(HashMap::new()),
            done: Mutex::new(Some(tx)),
            signer: Mutex::new(None),
        }),
        rx,
    )
}

/// The signing panel, rendered by the server that hosts the publish — the
/// landing page and /deploy of the preview interface both draw it from here.
/// Every fact is in the markup and the entity is minted NOW: the id printed is
/// the id the wallet signs, registered pending so `sign` recognises it. The
/// browser is left exactly one job, the wallet hand-off, which is the one
/// thing that cannot happen without JavaScript. `api` is the absolute
/// prefix-carrying path the script POSTs the signature to.
pub(crate) fn sign_section(st: &Arc<LinkerState>, api: &str) -> String {
    use crate::start::chrome::{esc, kv};
    let d = &st.dep;
    let ts = d.timestamp_override.unwrap_or_else(deploy::now_ms);
    let (entity_id, entity_bytes) = match deploy::build_entity(&d.prepared, ts) {
        Ok(x) => x,
        Err(e) => {
            return format!(
                r#"<div class="panel panel--warn" id="sign-panel"><h2>Cannot sign</h2><p class="note">could not build the entity: {}</p></div>"#,
                esc(&format!("{e:#}"))
            )
        }
    };
    let delete_payload = match d.needs_delete {
        true => d.world.as_deref().map(deploy::build_delete_payload),
        false => None,
    };
    {
        let mut pending = st.pending.lock().unwrap_or_else(PoisonError::into_inner);
        if pending.len() > 32 {
            pending.clear();
        }
        pending.insert(
            entity_id.clone(),
            PendingEntity {
                bytes: entity_bytes,
                delete_payload: delete_payload.clone(),
            },
        );
    }
    // The deep link is what actually reaches the realm this deploy lands in.
    // decentraland.org forwards `realm` only for realms it whitelists, so for
    // anything self-hosted its play URL silently drops the realm and boots
    // Genesis instead.
    let realm_url = match &d.world {
        Some(w) => catalyrst_types::world_realm_url(&d.target_content, w),
        None => d
            .target_content
            .trim_end_matches('/')
            .trim_end_matches("/content")
            .to_string(),
    };
    let deep_link = catalyrst_types::realm_deep_link(
        &realm_url,
        catalyrst_types::parse_position(Some(&d.base_parcel)),
    );
    let where_to = match &d.world {
        Some(w) if d.multi_scene => format!("world {w} (multi-scene, additive)"),
        Some(w) => format!("world {w}"),
        None => "Genesis City LAND".to_string(),
    };
    let total: usize = d.prepared.files.iter().map(|(_, _, b)| b.len()).sum();
    let delete_warn = match &delete_payload {
        Some(_) => {
            r#"<p class="note sign-warn">This deploy also REMOVES the scenes currently
    published on other parcels of the world; the wallet asks for a second signature authorizing
    the removal.</p>"#
        }
        None => "",
    };
    format!(
        r#"<div class="panel" id="sign-panel" data-api="{api}" data-entity-id="{id}"{delete_attr} data-deep-link="{deep}">
  <h2>Sign the deployment</h2>
  <span class="note">Connect the wallet that may publish this scene; nothing uploads until it answers.</span>
  <div class="kvs">
    {scene}{where_kv}{parcels}{entity}{payload}
  </div>
  {delete_warn}
  <button class="jn__cta" id="sign-go" type="button">Connect wallet and sign</button>
  <p class="note sign-status" id="sign-status" hidden></p>
  <noscript><p class="note">The wallet hand-off needs JavaScript; everything above is exact without it.</p></noscript>
</div>"#,
        api = esc(api),
        id = esc(&entity_id),
        delete_attr = match &delete_payload {
            Some(p) => format!(r#" data-delete-payload="{}""#, esc(p)),
            None => String::new(),
        },
        deep = esc(&deep_link),
        scene = kv("Scene", esc(&d.scene_title)),
        where_kv = kv("Deploying to", esc(&where_to)),
        // A handful of parcels earn their list; a big footprint is a count —
        // the same altitude the /deploy hint speaks at, and the layout map is
        // where the full shape lives.
        parcels = kv(
            "Parcels",
            esc(&if d.prepared.pointers.len() > 6 {
                format!(
                    "{} parcels · base {}",
                    d.prepared.pointers.len(),
                    d.base_parcel
                )
            } else {
                format!(
                    "{}  (base {})",
                    d.prepared.pointers.join("  "),
                    d.base_parcel
                )
            }),
        ),
        entity = kv("Entity", format!("<code>{}</code>", esc(&entity_id))),
        payload = kv(
            "Payload",
            esc(&format!(
                "{} files · {} · to {}",
                d.prepared.files.len(),
                deploy::human_size(total as u64),
                d.target_content
            )),
        ),
    )
}

#[derive(Deserialize)]
pub(crate) struct SignReq {
    address: String,
    signature: String,
    #[serde(rename = "entityId")]
    entity_id: String,
    #[serde(rename = "deleteSignature")]
    delete_signature: Option<String>,
}

pub(crate) async fn sign(
    State(st): State<Arc<LinkerState>>,
    Json(req): Json<SignReq>,
) -> Json<Value> {
    let pending = st
        .pending
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&req.entity_id);
    let Some(pending) = pending else {
        return Json(json!({
            "ok": false,
            "fatal": false,
            "error": "unknown or stale entity id — reload the page and sign again"
        }));
    };
    // Recorded before the outcome is known: even a refused upload was signed
    // by this wallet, and that is the fact the pages personalize on.
    *st.signer.lock().unwrap_or_else(PoisonError::into_inner) = Some(req.address.clone());
    if st.dep.check_permissions {
        if let Some(w) = st.dep.world.as_deref() {
            if let Err(e) = deploy::enforce_world_permission(
                &st.dep.target_content,
                w,
                &req.address,
                &st.dep.prepared.pointers,
            )
            .await
            {
                let msg = crate::ux::render(&e, false, false);
                finish(&st, Err(e));
                return Json(json!({ "ok": false, "fatal": true, "error": msg }));
            }
        }
    }
    if let Some(payload) = &pending.delete_payload {
        let Some(dsig) = &req.delete_signature else {
            return Json(json!({
                "ok": false,
                "fatal": false,
                "error": "this deploy also removes the existing world scenes and needs the second signature — reload and sign both prompts"
            }));
        };
        let chain = deploy::simple_auth_chain(&req.address, payload, dsig);
        if let Err(e) = deploy::send_world_delete(
            &st.dep.target_content,
            st.dep.world.as_deref().unwrap_or_default(),
            &chain,
        )
        .await
        {
            let msg = crate::ux::render(&e, false, false);
            finish(&st, Err(e));
            return Json(json!({ "ok": false, "fatal": true, "error": msg }));
        }
    }
    match deploy::upload_entity(
        &st.dep.target_content,
        &req.entity_id,
        pending.bytes.clone(),
        &st.dep.prepared.files,
        &req.address,
        &req.signature,
    )
    .await
    {
        Ok(message) => {
            if let Some(path) = &st.dep.entity_out {
                if let Err(e) = std::fs::write(path, &pending.bytes) {
                    tracing::warn!("could not write --entity-out {}: {e}", path.display());
                }
            }
            finish(&st, Ok(message.clone()));
            Json(json!({ "ok": true, "message": message }))
        }
        Err(e) => {
            let msg = crate::ux::render(&e, false, false);
            finish(&st, Err(e));
            Json(json!({ "ok": false, "fatal": true, "error": msg }))
        }
    }
}

fn finish(st: &Arc<LinkerState>, result: Result<String>) {
    let tx = st
        .done
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(tx) = tx {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            let _ = tx.send(result);
        });
    }
}

pub(crate) fn spawn_browser(url: &str) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let spawned = std::process::Command::new(program)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if spawned.is_err() {
        ux::note("could not open a browser automatically \u{2014} open the URL above manually");
    }
}

pub(crate) fn fmt_wait(timeout: Duration) -> String {
    let secs = timeout.as_secs();
    if secs >= 60 && secs.is_multiple_of(60) {
        let mins = secs / 60;
        if mins == 1 {
            "1 minute".to_string()
        } else {
            format!("{mins} minutes")
        }
    } else if secs == 1 {
        "1 second".to_string()
    } else {
        format!("{secs} seconds")
    }
}

fn timeout_error(timeout: Duration, url: &str) -> anyhow::Error {
    UserError::new(
        format!(
            "no signature arrived within {} \u{2014} deployment abandoned",
            fmt_wait(timeout)
        ),
        TrySteps::one("re-run dcl-one-sdk deploy and sign on the printed URL")
            .and("raise the wait with DCL_ONE_SDK_LINKER_TIMEOUT_SECS=<seconds>")
            .and("for headless deploys set DCL_PRIVATE_KEY or pass --sign-key <file>"),
    )
    .why(format!("the signing page was served at {url}"))
    .into()
}

pub fn linker_bind_host() -> String {
    std::env::var("DCL_ONE_SDK_LINKER_HOST")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

pub(crate) async fn await_outcome(
    rx: tokio::sync::oneshot::Receiver<Result<String>>,
    timeout: Duration,
    url: &str,
) -> Result<String> {
    tokio::select! {
        res = rx => match res {
            Ok(outcome) => outcome,
            Err(_) => Err(UserError::new(
                "the signing flow ended without a result",
                TrySteps::one("re-run dcl-one-sdk deploy"),
            )
            .into()),
        },
        _ = tokio::time::sleep(timeout) => Err(timeout_error(timeout, url)),
    }
}

pub async fn run(dep: LinkerDeploy, opts: LinkerOptions) -> Result<String> {
    let dir = dep.dir.clone();
    let (state, rx) = new_state(dep);
    if let Some(host) = opts.host {
        // No terminal narration: the hosting page owns the hand-off, and its
        // URL printed into the preview terminal was the noise a page-driven
        // publish used to make.
        (host.register)(state);
        return await_outcome(rx, opts.timeout, &host.url).await;
    }
    crate::start::serve_signing(&dir, opts.port, opts.open_browser, opts.timeout, state, rx).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Project;
    use std::path::Path;

    struct TempTree(PathBuf);

    impl TempTree {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "dcl-one-sdk-linker-test-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempTree(dir)
        }

        fn write(&self, rel: &str, contents: &str) {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, contents).unwrap();
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture(tag: &str, world: Option<&str>) -> (TempTree, LinkerDeploy) {
        let t = TempTree::new(tag);
        let world_cfg = match world {
            Some(w) => format!(",\"worldConfiguration\":{{\"name\":\"{w}\"}}"),
            None => String::new(),
        };
        t.write(
            "scene.json",
            &format!(
                "{{\"runtimeVersion\":\"7\",\"main\":\"bin/index.js\",\"display\":{{\"title\":\"Linker Smoke\"}},\"scene\":{{\"parcels\":[\"0,0\"],\"base\":\"0,0\"}}{world_cfg}}}"
            ),
        );
        t.write("bin/index.js", "console.log(\"linker\");\n");
        let project = Project::load(&t.0).unwrap();
        let prepared = deploy::prepare(&project).unwrap();
        let dep = LinkerDeploy {
            dir: t.0.clone(),
            prepared,
            target_content: "http://127.0.0.1:9".to_string(),
            world: world.map(str::to_string),
            needs_delete: false,
            timestamp_override: None,
            entity_out: None,
            scene_title: "Linker Smoke".to_string(),
            base_parcel: "0,0".to_string(),
            multi_scene: false,
            check_permissions: false,
        };
        (t, dep)
    }

    /// The id the rendered panel carries — which is also the pending key,
    /// because minting happens at render time.
    fn minted_entity_id(section: &str) -> String {
        let marker = r#"data-entity-id=""#;
        let at = section.find(marker).expect("panel carries the entity id") + marker.len();
        section[at..].split('"').next().unwrap().to_string()
    }

    #[test]
    fn linker_bind_host_defaults_to_loopback() {
        std::env::remove_var("DCL_ONE_SDK_LINKER_HOST");
        assert_eq!(linker_bind_host(), "127.0.0.1");
        std::env::set_var("DCL_ONE_SDK_LINKER_HOST", "0.0.0.0");
        assert_eq!(linker_bind_host(), "0.0.0.0");
        std::env::remove_var("DCL_ONE_SDK_LINKER_HOST");
    }

    #[tokio::test]
    async fn section_mints_pending_and_stale_sign_answers_nonfatal() {
        let (_t, dep) = fixture("smoke", None);
        let (state, _rx) = new_state(dep);

        let section = sign_section(&state, "/deploy/sign");
        assert!(section.contains(r#"data-api="/deploy/sign""#));
        assert!(section.contains("Linker Smoke"));
        assert!(
            section.contains("2 files"),
            "payload row is server-rendered"
        );
        assert!(!section.contains("data-delete-payload"));
        let id = minted_entity_id(&section);
        assert!(id.starts_with("bafkrei"));
        assert!(
            state.pending.lock().unwrap().contains_key(&id),
            "the rendered id is registered pending"
        );

        let stale = sign(
            State(state.clone()),
            Json(SignReq {
                address: "0x0".into(),
                signature: "0x0".into(),
                entity_id: "bogus".into(),
                delete_signature: None,
            }),
        )
        .await;
        assert_eq!(stale.0["ok"], false);
        assert_eq!(stale.0["fatal"], false);
    }

    #[tokio::test]
    async fn unreachable_target_fails_fatal_and_resolves_cli() {
        let (_t, dep) = fixture("fatal", None);
        let (state, rx) = new_state(dep);
        let entity_id = minted_entity_id(&sign_section(&state, "/deploy/sign"));
        let signer = crate::random_test_wallet();
        let sig = signer.sign_message(entity_id.as_bytes()).unwrap();
        let resp = sign(
            State(state.clone()),
            Json(SignReq {
                address: signer.address(),
                signature: sig,
                entity_id,
                delete_signature: None,
            }),
        )
        .await;
        assert_eq!(resp.0["ok"], false);
        assert_eq!(resp.0["fatal"], true);
        let outcome = rx.await.unwrap();
        assert!(outcome.is_err());
    }

    #[tokio::test]
    async fn sign_flow_completes_against_local_worlds() {
        let Ok(key_path) = std::env::var("DCL_ONE_SDK_LINKER_SMOKE_KEY") else {
            eprintln!("skipping: DCL_ONE_SDK_LINKER_SMOKE_KEY not set");
            return;
        };
        let target = std::env::var("DCL_ONE_SDK_LINKER_SMOKE_TARGET")
            .unwrap_or_else(|_| "http://127.0.0.1:5142".to_string());
        let world = std::env::var("DCL_ONE_SDK_LINKER_SMOKE_WORLD")
            .unwrap_or_else(|_| "dcl1test.dcl.eth".to_string());
        let raw = std::fs::read_to_string(Path::new(&key_path)).unwrap();
        let signer = catalyrst_crypto::Wallet::from_hex(&raw).unwrap();

        let (_t, mut dep) = fixture("worlds", Some(&world));
        dep.target_content = target;
        let (state, rx) = new_state(dep);
        let entity_id = minted_entity_id(&sign_section(&state, "/deploy/sign"));
        let sig = signer.sign_message(entity_id.as_bytes()).unwrap();
        let resp = sign(
            State(state.clone()),
            Json(SignReq {
                address: signer.address(),
                signature: sig,
                entity_id,
                delete_signature: None,
            }),
        )
        .await;
        assert_eq!(resp.0["ok"], true, "sign flow failed: {}", resp.0);
        let outcome = rx.await.unwrap();
        let message = outcome.unwrap();
        assert!(
            message.contains("Deployed"),
            "unexpected message: {message}"
        );
    }
}
