//! LiveKit rooms for the preview realm.
//!
//! A preview's comms live on rooms of a LiveKit SFU, which is what carries
//! voice in both explorers: by default the livekit-server `start` runs itself
//! ([`crate::livekit_server`]), or with `--livekit-url` one elsewhere. Either
//! way the preview holds the SFU's API key and secret and mints one join
//! token per wallet and room the way the production gatekeeper does: an HS256
//! JWT whose `iss` is the key, whose `sub` is the wallet, and whose `video`
//! grant names the room. No LiveKit SDK is involved; the SFU's
//! `/rtc/validate` accepts what `token` produces. `--no-livekit` keeps the
//! built-in ws-room (positions, chat and scene messages, no audio).

use crate::ux::{self, TrySteps, UserError};
use anyhow::{bail, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use serde_json::json;
use sha2::Sha256;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A token is checked when the explorer joins, and again on a full
/// reconnect; an hour covers a dropped connection without a client having to
/// re-sign, and the ws-room preview trusts the same peers indefinitely.
pub const TOKEN_TTL_SECS: i64 = 60 * 60;

/// The same names the `lk` CLI reads, so a shell set up for it is set up for
/// the preview too.
pub const URL_ENV: &str = "LIVEKIT_URL";
pub const API_KEY_ENV: &str = "LIVEKIT_API_KEY";
pub const API_SECRET_ENV: &str = "LIVEKIT_API_SECRET";

pub const DEFAULT_ROOM: &str = "LocalPreview";

/// Where the SFU is, from an explorer's point of view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    /// `ws://host:port` or `wss://host`, no trailing slash, as `--livekit-url`
    /// gave it: the same for every peer, and reachable from each peer's
    /// machine, not just from this one.
    Fixed(String),
    /// The server `start` runs itself, bound on every interface of this
    /// machine on `port`. A peer reaches it on whatever name or address it
    /// reached the preview on, so its adapter is built per request.
    Embedded { port: u16 },
}

/// The SFU the preview mints tokens for.
#[derive(Clone)]
pub struct Livekit {
    pub endpoint: Endpoint,
    pub api_key: String,
    api_secret: String,
    /// The realm room; scene rooms are `scene:<room>:<sceneId>`, the shape
    /// the production gatekeeper uses, so two previews on one SFU stay apart
    /// by renaming this.
    pub room: String,
    pub ttl_secs: i64,
}

impl fmt::Debug for Livekit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Livekit")
            .field("endpoint", &self.endpoint)
            .field("api_key", &self.api_key)
            .field("api_secret", &"<redacted>")
            .field("room", &self.room)
            .field("ttl_secs", &self.ttl_secs)
            .finish()
    }
}

impl Livekit {
    /// `url` may be the SFU's HTTP address (`http://127.0.0.1:7880`, what
    /// `livekit-server` prints); it is normalised to the websocket form the
    /// adapter string needs.
    pub fn new(url: &str, api_key: &str, api_secret: &str, room: &str) -> Result<Self> {
        Self::checked(
            Endpoint::Fixed(normalize_url(url)?),
            api_key,
            api_secret,
            room,
        )
    }

    /// The server `start` runs itself, on `port` of this machine.
    pub fn embedded(port: u16, api_key: &str, api_secret: &str, room: &str) -> Result<Self> {
        Self::checked(Endpoint::Embedded { port }, api_key, api_secret, room)
    }

    fn checked(endpoint: Endpoint, api_key: &str, api_secret: &str, room: &str) -> Result<Self> {
        if api_key.trim().is_empty() {
            bail!("the LiveKit API key is empty (--livekit-api-key or {API_KEY_ENV})");
        }
        if api_secret.trim().is_empty() {
            bail!("the LiveKit API secret is empty (--livekit-api-secret, --livekit-api-secret-file or {API_SECRET_ENV})");
        }
        let room = validate_room(room)?;
        Ok(Livekit {
            endpoint,
            api_key: api_key.trim().to_string(),
            api_secret: api_secret.trim().to_string(),
            room: room.to_string(),
            ttl_secs: TOKEN_TTL_SECS,
        })
    }

    /// The SFU as the peer that reached the preview on `preview_host` (its
    /// `Host` header, port and all) reaches it: the fixed URL, or the
    /// embedded server on that same host.
    pub fn url_for(&self, preview_host: &str) -> String {
        match &self.endpoint {
            Endpoint::Fixed(url) => url.clone(),
            Endpoint::Embedded { port } => {
                format!("ws://{}:{port}", host_without_port(preview_host))
            }
        }
    }

    /// The SFU as this machine reaches it, for the banner.
    pub fn describe_url(&self) -> String {
        self.url_for("127.0.0.1")
    }

    pub fn realm_room(&self) -> &str {
        &self.room
    }

    pub fn scene_room(&self, scene_id: &str) -> String {
        format!("scene:{}:{scene_id}", self.room)
    }

    /// A join token for `identity` in `room`: publish + subscribe + data,
    /// valid from ten seconds ago (clock skew) for `ttl_secs`.
    pub fn token(&self, identity: &str, room: &str) -> String {
        self.token_at(identity, room, unix_now())
    }

    fn token_at(&self, identity: &str, room: &str, now: i64) -> String {
        let header = b64url(br#"{"alg":"HS256","typ":"JWT"}"#);
        let payload = json!({
            "iss": self.api_key,
            "sub": identity,
            "nbf": now - 10,
            "exp": now + self.ttl_secs,
            "video": {
                "room": room,
                "roomJoin": true,
                "canPublish": true,
                "canSubscribe": true,
                "canPublishData": true,
            },
        });
        let payload = b64url(payload.to_string().as_bytes());
        let signing_input = format!("{header}.{payload}");
        let mut mac = Hmac::<Sha256>::new_from_slice(self.api_secret.as_bytes())
            .expect("HMAC accepts a key of any length");
        mac.update(signing_input.as_bytes());
        let sig = b64url(&mac.finalize().into_bytes());
        format!("{signing_input}.{sig}")
    }

    /// The RFC-5 adapter string an explorer that reached the preview on
    /// `preview_host` joins `room` with as `identity`.
    pub fn adapter(&self, identity: &str, room: &str, preview_host: &str) -> String {
        format!(
            "livekit:{}?access_token={}",
            self.url_for(preview_host),
            self.token(identity, room)
        )
    }
}

/// A room name LiveKit and the URL-ish places it lands in both accept.
pub fn validate_room(room: &str) -> Result<&str> {
    let room = room.trim();
    if room.is_empty()
        || !room
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        bail!("the LiveKit room name {room:?} must be letters, digits, '-', '_' or '.'");
    }
    Ok(room)
}

/// `host[:port]` without the port; an IPv6 literal keeps its brackets.
fn host_without_port(host: &str) -> String {
    let host = host.trim();
    let bare = if let Some(rest) = host.strip_prefix('[') {
        match rest.find(']') {
            Some(end) => format!("[{}]", &rest[..end]),
            None => host.to_string(),
        }
    } else {
        match host.rsplit_once(':') {
            Some((name, port))
                if !port.is_empty()
                    && port.bytes().all(|b| b.is_ascii_digit())
                    && !name.contains(':') =>
            {
                name.to_string()
            }
            _ => host.to_string(),
        }
    };
    if bare.is_empty() {
        "127.0.0.1".to_string()
    } else {
        bare
    }
}

/// What `start` was given for LiveKit, flags only; the env fills the gaps.
#[derive(Debug, Default)]
pub struct CliArgs {
    pub url: Option<String>,
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub api_secret_file: Option<PathBuf>,
    pub room: String,
    pub offline_comms: bool,
}

/// `Ok(None)` is a preview on the built-in ws-room. A flag makes the rest
/// mandatory; an env that is only partly set is noted and ignored, so a
/// shell that exports `LIVEKIT_URL` for the `lk` CLI does not break `start`.
pub fn resolve(args: CliArgs) -> Result<Option<Livekit>> {
    let any_flag = args.url.is_some()
        || args.api_key.is_some()
        || args.api_secret.is_some()
        || args.api_secret_file.is_some();
    let url = args.url.or_else(|| env_var(URL_ENV));
    let api_key = args.api_key.or_else(|| env_var(API_KEY_ENV));
    let api_secret = match (args.api_secret, args.api_secret_file.as_deref()) {
        (Some(s), _) => Some(s),
        (None, Some(path)) => Some(read_secret_file(path)?),
        (None, None) => env_var(API_SECRET_ENV),
    };
    let missing: Vec<&str> = [
        (url.is_none(), "--livekit-url"),
        (api_key.is_none(), "--livekit-api-key"),
        (api_secret.is_none(), "--livekit-api-secret"),
    ]
    .into_iter()
    .filter_map(|(absent, flag)| absent.then_some(flag))
    .collect();
    if missing.len() == 3 {
        return Ok(None);
    }
    if !missing.is_empty() {
        if !any_flag {
            ux::note_stderr(format!(
                "{URL_ENV}/{API_KEY_ENV}/{API_SECRET_ENV} are only partly set; comms stay on the built-in ws-room (no voice)"
            ));
            return Ok(None);
        }
        return Err(UserError::new(
            format!("LiveKit is only partly configured: missing {}", missing.join(", ")),
            TrySteps::one(
                "give all of --livekit-url, --livekit-api-key and --livekit-api-secret (or --livekit-api-secret-file)",
            )
            .and(format!("or export {URL_ENV}, {API_KEY_ENV} and {API_SECRET_ENV}")),
        )
        .into());
    }
    if args.offline_comms {
        return Err(UserError::new(
            "--offline-comms and a LiveKit server contradict each other",
            TrySteps::one("drop --offline-comms to put comms and voice on LiveKit")
                .and("or drop the --livekit-* flags (and unset LIVEKIT_URL) for no comms at all"),
        )
        .into());
    }
    let livekit = Livekit::new(
        url.as_deref().unwrap_or_default(),
        api_key.as_deref().unwrap_or_default(),
        api_secret.as_deref().unwrap_or_default(),
        &args.room,
    )?;
    if !any_flag {
        ux::note_stderr(format!("using the LiveKit server from {URL_ENV}"));
    }
    Ok(Some(livekit))
}

fn env_var(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn read_secret_file(path: &Path) -> Result<String> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        anyhow::Error::from(
            UserError::new(
                format!("could not read the LiveKit secret file {}", path.display()),
                TrySteps::one("check the --livekit-api-secret-file path"),
            )
            .caused_by(e),
        )
    })?;
    Ok(raw.trim().to_string())
}

fn normalize_url(url: &str) -> Result<String> {
    let url = url.trim().trim_end_matches('/');
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r),
        None => {
            bail!("the LiveKit URL {url:?} has no scheme; expected ws://host:port or wss://host")
        }
    };
    let scheme = match scheme.as_str() {
        "ws" | "wss" => scheme,
        "http" => "ws".to_string(),
        "https" => "wss".to_string(),
        other => bail!("the LiveKit URL scheme {other:?} is not ws/wss (or http/https)"),
    };
    if rest.is_empty() || rest.starts_with('/') {
        bail!("the LiveKit URL {url:?} names no host");
    }
    Ok(format!("{scheme}://{rest}"))
}

fn b64url(raw: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(raw)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn lk() -> Livekit {
        Livekit::new("ws://127.0.0.1:7880", "devkey", "secret", DEFAULT_ROOM).unwrap()
    }

    fn decode(part: &str) -> Value {
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
    }

    #[test]
    fn token_carries_the_room_grant_for_the_identity() {
        let tok = lk().token_at("0xabc", "room-1", 1_000_000);
        let parts: Vec<&str> = tok.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(decode(parts[0]), json!({"alg": "HS256", "typ": "JWT"}));
        let payload = decode(parts[1]);
        assert_eq!(payload["iss"], "devkey");
        assert_eq!(payload["sub"], "0xabc");
        assert_eq!(payload["nbf"], 1_000_000 - 10);
        assert_eq!(payload["exp"], 1_000_000 + TOKEN_TTL_SECS);
        assert_eq!(
            payload["video"],
            json!({
                "room": "room-1",
                "roomJoin": true,
                "canPublish": true,
                "canSubscribe": true,
                "canPublishData": true
            })
        );
    }

    #[test]
    fn signature_is_hs256_over_header_dot_payload() {
        let tok = lk().token_at("0xabc", "room-1", 1_000_000);
        let (signing_input, sig) = tok.rsplit_once('.').unwrap();
        let mut mac = Hmac::<Sha256>::new_from_slice(b"secret").unwrap();
        mac.update(signing_input.as_bytes());
        assert_eq!(b64url(&mac.finalize().into_bytes()), sig);
        let mut wrong = Hmac::<Sha256>::new_from_slice(b"other").unwrap();
        wrong.update(signing_input.as_bytes());
        assert_ne!(b64url(&wrong.finalize().into_bytes()), sig);
    }

    #[test]
    fn an_embedded_server_is_reached_on_the_host_the_preview_was() {
        let emb = Livekit::embedded(7880, "dcl-one-sdk", "secret", DEFAULT_ROOM).unwrap();
        for (preview_host, want) in [
            ("127.0.0.1:8000", "ws://127.0.0.1:7880"),
            ("192.0.2.20:8001", "ws://192.0.2.20:7880"),
            ("preview.local", "ws://preview.local:7880"),
            ("[::1]:8000", "ws://[::1]:7880"),
            ("[fe80::1]", "ws://[fe80::1]:7880"),
            ("", "ws://127.0.0.1:7880"),
        ] {
            assert_eq!(emb.url_for(preview_host), want, "for {preview_host:?}");
        }
        assert_eq!(emb.describe_url(), "ws://127.0.0.1:7880");
        let a = emb.adapter("0xabc", "LocalPreview", "198.51.100.2:8000");
        assert!(
            a.starts_with("livekit:ws://198.51.100.2:7880?access_token=eyJ"),
            "{a}"
        );
        assert_eq!(
            lk().url_for("198.51.100.2:8000"),
            "ws://127.0.0.1:7880",
            "a fixed URL ignores the request host"
        );
    }

    #[test]
    fn adapter_is_the_rfc5_livekit_form() {
        let a = lk().adapter("0xabc", &lk().scene_room("bafk"), "127.0.0.1:8000");
        assert!(
            a.starts_with("livekit:ws://127.0.0.1:7880?access_token=eyJ"),
            "{a}"
        );
        assert_eq!(a.matches("?access_token=").count(), 1);
        assert_eq!(lk().realm_room(), "LocalPreview");
        assert_eq!(lk().scene_room("bafk"), "scene:LocalPreview:bafk");
    }

    #[test]
    fn urls_are_normalised_to_websocket_form() {
        for (given, want) in [
            ("http://127.0.0.1:7880", "ws://127.0.0.1:7880"),
            ("https://sfu.example/", "wss://sfu.example"),
            ("WSS://sfu.example", "wss://sfu.example"),
            ("ws://10.0.0.5:7880/", "ws://10.0.0.5:7880"),
        ] {
            assert_eq!(
                Livekit::new(given, "k", "s", "r").unwrap().describe_url(),
                want,
                "{given}"
            );
        }
        for bad in ["127.0.0.1:7880", "ftp://x", "ws://", "ws:///path"] {
            assert!(Livekit::new(bad, "k", "s", "r").is_err(), "{bad}");
        }
        assert!(Livekit::new("ws://x", "", "s", "r").is_err());
        assert!(Livekit::new("ws://x", "k", " ", "r").is_err());
        assert!(Livekit::new("ws://x", "k", "s", "").is_err());
        assert!(Livekit::new("ws://x", "k", "s", "a room").is_err());
        assert!(Livekit::new("ws://x", "k", "s", "scene:x").is_err());
    }

    #[test]
    fn debug_never_prints_the_secret() {
        let s = format!("{:?}", lk());
        assert!(s.contains("devkey"));
        assert!(!s.contains("secret\""), "{s}");
        assert!(s.contains("<redacted>"));
    }

    #[test]
    fn resolve_needs_all_three_once_a_flag_is_given() {
        let args = |url: Option<&str>, key: Option<&str>, secret: Option<&str>| CliArgs {
            url: url.map(str::to_string),
            api_key: key.map(str::to_string),
            api_secret: secret.map(str::to_string),
            api_secret_file: None,
            room: DEFAULT_ROOM.to_string(),
            offline_comms: false,
        };
        let full = resolve(args(Some("ws://x:7880"), Some("k"), Some("s")))
            .unwrap()
            .expect("configured");
        assert_eq!(full.endpoint, Endpoint::Fixed("ws://x:7880".into()));
        let err = resolve(args(Some("ws://x:7880"), None, Some("s"))).unwrap_err();
        assert!(err.to_string().contains("--livekit-api-key"), "{err}");
        let mut offline = args(Some("ws://x:7880"), Some("k"), Some("s"));
        offline.offline_comms = true;
        assert!(resolve(offline)
            .unwrap_err()
            .to_string()
            .contains("--offline-comms"));
    }

    #[test]
    fn resolve_reads_the_secret_file_trimmed() {
        let dir = std::env::temp_dir().join(format!("dcl-one-sdk-lk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("secret");
        std::fs::write(&file, "  s3cret\n").unwrap();
        let lk = resolve(CliArgs {
            url: Some("ws://x:7880".into()),
            api_key: Some("k".into()),
            api_secret: None,
            api_secret_file: Some(file.clone()),
            room: "r".into(),
            offline_comms: false,
        })
        .unwrap()
        .unwrap();
        let tok = lk.token_at("0x1", "r", 1);
        let (signing_input, sig) = tok.rsplit_once('.').unwrap();
        let mut mac = Hmac::<Sha256>::new_from_slice(b"s3cret").unwrap();
        mac.update(signing_input.as_bytes());
        assert_eq!(b64url(&mac.finalize().into_bytes()), sig);
        std::fs::remove_dir_all(&dir).unwrap();
        let missing = resolve(CliArgs {
            url: Some("ws://x:7880".into()),
            api_key: Some("k".into()),
            api_secret: None,
            api_secret_file: Some(file),
            room: "r".into(),
            offline_comms: false,
        });
        assert!(missing.unwrap_err().to_string().contains("secret file"));
    }
}
