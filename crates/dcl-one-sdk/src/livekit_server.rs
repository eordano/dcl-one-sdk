//! The LiveKit server a preview runs by default: what turns its comms rooms
//! into voice rooms with nothing installed. One process per preview with a
//! random API secret of its own, on 7880/7881/7882 (signalling, media over
//! TCP, media over UDP) or the next free trio — 7883/7884/7885 beside a running
//! system LiveKit — killed with the preview.
//! `start --livekit-url` names a server elsewhere instead; `--no-livekit`
//! keeps the built-in ws-room.

use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 7880;
/// The API key the preview mints with; the secret is per run.
pub const API_KEY: &str = "dcl-one-sdk";
/// Set to run a livekit-server other than the embedded one (or the one on
/// PATH, when this build embeds none).
pub const BIN_ENV: &str = "LIVEKIT_SERVER_BIN";
const READY_TIMEOUT: Duration = Duration::from_secs(15);
const READY_POLL: Duration = Duration::from_millis(200);
/// How far past `DEFAULT_PORT` a busy machine is searched for a free trio.
const PORT_SEARCH: u16 = 200;

/// A server that answered its health check. Dropping it kills the process;
/// [`kill_group`] reaches anything it spawned as well.
pub struct Running {
    pub bin: PathBuf,
    /// "embedded", "PATH" or the env var that named the binary.
    pub source: &'static str,
    pub port: u16,
    pub tcp_port: u16,
    pub udp_port: u16,
    api_secret: String,
    _child: tokio::process::Child,
}

impl Running {
    pub fn api_secret(&self) -> &str {
        &self.api_secret
    }
}

#[cfg(unix)]
static PGID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// The process group of the running server, for the way out of `start`:
/// `kill_on_drop` fires on a clean drop only.
pub fn kill_group() {
    #[cfg(unix)]
    crate::asset_bundles::kill_process_group(PGID.swap(0, std::sync::atomic::Ordering::SeqCst));
}

/// Which livekit-server to run: the one named in `LIVEKIT_SERVER_BIN`, else
/// the embedded one, else one on PATH (brew's on macOS).
pub fn resolve_bin() -> Option<(PathBuf, &'static str)> {
    resolve_bin_from(std::env::var(BIN_ENV).ok())
}

fn resolve_bin_from(named: Option<String>) -> Option<(PathBuf, &'static str)> {
    if let Some(named) = named
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        return Some((PathBuf::from(named), BIN_ENV));
    }
    if let Some(embedded) = crate::livekit_embed::ensure_extracted() {
        return Some((embedded, "embedded"));
    }
    find_on_path("livekit-server").map(|p| (p, "PATH"))
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let names: &[String] = &if cfg!(windows) {
        vec![format!("{name}.exe"), name.to_string()]
    } else {
        vec![name.to_string()]
    };
    std::env::split_paths(&path).find_map(|dir| {
        names
            .iter()
            .map(|n| dir.join(n))
            .find(|candidate| candidate.is_file())
    })
}

/// The first `base` at or after `first` with `base` and `base+1` free over
/// TCP and `base+2` free over UDP, on every interface: the server binds all
/// three on 0.0.0.0 so LAN peers can reach it.
pub fn pick_ports(first: u16) -> Option<(u16, u16, u16)> {
    (first..first.saturating_add(PORT_SEARCH))
        .filter(|base| base.checked_add(2).is_some())
        .find(|&base| tcp_free(base) && tcp_free(base + 1) && udp_free(base + 2))
        .map(|base| (base, base + 1, base + 2))
}

fn tcp_free(port: u16) -> bool {
    std::net::TcpListener::bind(("0.0.0.0", port)).is_ok()
}

fn udp_free(port: u16) -> bool {
    std::net::UdpSocket::bind(("0.0.0.0", port)).is_ok()
}

/// livekit-server's YAML, passed whole through `LIVEKIT_CONFIG` so the secret
/// never shows in `ps`. No STUN (`use_external_ip: false`): peers are on this
/// machine or its LAN, and a public server is `--livekit-url`'s job.
pub fn config_yaml(
    port: u16,
    tcp_port: u16,
    udp_port: u16,
    api_key: &str,
    api_secret: &str,
) -> String {
    format!(
        "port: {port}\n\
         bind_addresses:\n  - \"0.0.0.0\"\n\
         rtc:\n  tcp_port: {tcp_port}\n  udp_port: {udp_port}\n  use_external_ip: false\n\
         keys:\n  {api_key}: {api_secret}\n\
         logging:\n  level: warn\n"
    )
}

fn random_secret() -> String {
    let bytes: [u8; 32] = rand::random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Starts the server and waits for its health check. Every way this can
/// fail is said on stderr once, and the caller keeps the built-in ws-room.
pub async fn spawn(first_port: u16) -> Option<Running> {
    let Some((bin, source)) = resolve_bin() else {
        crate::ux::note(
            "voice off \u{2014} this build embeds no livekit-server for this platform and PATH has none \
             (brew install livekit, or set LIVEKIT_SERVER_BIN, or pass --livekit-url); comms stay on the built-in ws-room",
        );
        return None;
    };
    let Some((port, tcp_port, udp_port)) = pick_ports(first_port) else {
        crate::ux::note_stderr(format!(
            "voice off \u{2014} no free port trio from {first_port} up for livekit-server; comms stay on the built-in ws-room"
        ));
        return None;
    };
    let api_secret = random_secret();
    let mut cmd = tokio::process::Command::new(&bin);
    #[cfg(unix)]
    cmd.process_group(0);
    die_with_parent(&mut cmd);
    cmd.env(
        "LIVEKIT_CONFIG",
        config_yaml(port, tcp_port, udp_port, API_KEY, &api_secret),
    )
    .env_remove("LIVEKIT_KEYS")
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .kill_on_drop(true);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            crate::ux::note_stderr(format!(
                "voice off \u{2014} {} failed to start: {e}; comms stay on the built-in ws-room",
                bin.display()
            ));
            return None;
        }
    };
    if let Some(out) = child.stdout.take() {
        relay(out);
    }
    if let Some(err) = child.stderr.take() {
        relay(err);
    }
    #[cfg(unix)]
    PGID.store(
        child.id().map(|id| id as i32).unwrap_or(0),
        std::sync::atomic::Ordering::SeqCst,
    );

    let health = format!("http://127.0.0.1:{port}/");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .ok()?;
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            crate::ux::note_stderr(format!(
                "voice off \u{2014} {} exited ({status}) before answering on {health}; comms stay on the built-in ws-room",
                bin.display()
            ));
            return None;
        }
        if let Ok(res) = client.get(&health).send().await {
            if res.status().is_success() {
                break;
            }
        }
        if Instant::now() >= deadline {
            crate::ux::note_stderr(format!(
                "voice off \u{2014} {} did not answer on {health} within {}; comms stay on the built-in ws-room",
                bin.display(),
                crate::ux::fmt_elapsed(READY_TIMEOUT)
            ));
            kill_group();
            return None;
        }
        tokio::time::sleep(READY_POLL).await;
    }
    Some(Running {
        bin,
        source,
        port,
        tcp_port,
        udp_port,
        api_secret,
        _child: child,
    })
}

/// A preview that is killed outright (SIGKILL, a crash, a test harness'
/// `Child::kill`) never reaches `kill_group`; on Linux the kernel then
/// delivers this signal to the server for it, so no livekit-server outlives
/// its preview holding the port trio. Elsewhere `kill_on_drop` and the
/// process group are all there is.
#[cfg(target_os = "linux")]
fn die_with_parent(cmd: &mut tokio::process::Command) {
    // SAFETY: prctl is async-signal-safe and touches no state of ours.
    unsafe {
        cmd.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
}

#[cfg(not(target_os = "linux"))]
fn die_with_parent(_cmd: &mut tokio::process::Command) {}

/// The server's own log, at warn level: a refused token or a media port it
/// could not open is worth a line, everything else only under --verbose.
fn relay<R>(reader: R)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    use tokio::io::AsyncBufReadExt;
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if crate::ux::verbose() || line.contains("WARN") || line.contains("ERROR") {
                crate::ux::note_stderr(format!("livekit: {}", line.trim_end()));
            }
        }
    });
}

/// What the banner says about a running server.
pub fn describe(running: &Running, version: &str) -> String {
    let which = match running.source {
        "embedded" => format!("embedded livekit-server {version}"),
        other => format!("livekit-server from {other}"),
    };
    format!(
        "{which} on port {} (media: tcp {}, udp {})",
        running.port, running.tcp_port, running.udp_port
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_carries_ports_key_and_secret_at_warn_level() {
        let yaml = config_yaml(7880, 7881, 7882, "dcl-one-sdk", "s3cret");
        for line in [
            "port: 7880",
            "  tcp_port: 7881",
            "  udp_port: 7882",
            "  use_external_ip: false",
            "  dcl-one-sdk: s3cret",
            "  level: warn",
            "  - \"0.0.0.0\"",
        ] {
            assert!(yaml.contains(line), "missing {line:?} in:\n{yaml}");
        }
    }

    #[test]
    fn a_taken_port_moves_the_whole_trio_up() {
        let hold = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let taken = hold.local_addr().unwrap().port();
        if taken > u16::MAX - 4 {
            return;
        }
        let (p, t, u) = pick_ports(taken).expect("some trio above the held port");
        assert!(p > taken, "{p} must skip the held {taken}");
        assert_eq!((t, u), (p + 1, p + 2));
        let hold_udp = std::net::UdpSocket::bind(("0.0.0.0", p + 2)).unwrap();
        let (p2, _, _) = pick_ports(taken).expect("another trio");
        assert!(
            p2 > p,
            "{p2} must skip the trio whose udp port {} is held",
            p + 2
        );
        drop(hold_udp);
    }

    #[test]
    fn secrets_are_64_hex_chars_and_never_repeat() {
        let a = random_secret();
        let b = random_secret();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn a_named_binary_wins_over_the_embedded_server() {
        let (bin, source) = resolve_bin_from(Some(" /opt/lk/livekit-server ".into())).unwrap();
        assert_eq!(bin, PathBuf::from("/opt/lk/livekit-server"));
        assert_eq!(source, BIN_ENV);
        let unnamed = resolve_bin_from(Some("   ".into()));
        assert_eq!(
            unnamed.map(|(_, s)| s),
            resolve_bin_from(None).map(|(_, s)| s),
            "a blank name is no name"
        );
    }

    #[test]
    fn a_missing_name_on_path_is_none() {
        assert_eq!(find_on_path("dcl-one-sdk-no-such-binary-xyz"), None);
    }
}
