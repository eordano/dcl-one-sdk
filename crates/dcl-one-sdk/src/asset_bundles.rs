use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const READY_TIMEOUT: Duration = Duration::from_secs(15);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DEFAULT_CATALYST: &str = "https://peer.decentraland.org";
const INSTALL_HINT: &str =
    "install the @dcl/abgen npm package or set ABGEN_BIN; --no-asset-bundles silences this";

fn env_or(name: &str, default: String) -> String {
    match std::env::var(name) {
        Ok(v) if !v.is_empty() => v,
        _ => default,
    }
}

fn free_port() -> Option<u16> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).ok()?;
    Some(l.local_addr().ok()?.port())
}

fn npm_bin(project_root: &Path) -> Option<PathBuf> {
    let platform = if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return None;
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        return None;
    };
    let bin = if cfg!(target_os = "windows") {
        "abgen.exe"
    } else {
        "abgen"
    };
    let p = project_root
        .join("node_modules")
        .join("@dcl")
        .join(format!("abgen-{platform}-{arch}"))
        .join(bin);
    p.is_file().then_some(p)
}

fn pick_bin(env_bin: Option<String>, embedded: Option<PathBuf>, npm: Option<PathBuf>) -> String {
    if let Some(v) = env_bin.filter(|v| !v.is_empty()) {
        return v;
    }
    if let Some(p) = embedded.or(npm) {
        return p.display().to_string();
    }
    "abgen".to_string()
}

pub fn resolve_bin(project_root: &Path) -> String {
    pick_bin(
        std::env::var("ABGEN_BIN").ok(),
        crate::abgen_embed::ensure_extracted(),
        npm_bin(project_root),
    )
}

pub struct Sidecar {
    pub url: String,
    pub bin: String,
    exited: tokio::sync::watch::Receiver<bool>,
}

pub fn spawn_sidecar(preview_port: u16, project_root: &Path) -> Option<Sidecar> {
    let bin = resolve_bin(project_root);
    let port = free_port()?;
    let url = format!("http://127.0.0.1:{port}");
    let cache_root: PathBuf = std::env::temp_dir().join("dcl-abgen");

    let spawned = tokio::process::Command::new(&bin)
        .env("HTTP_SERVER_HOST", "127.0.0.1")
        .env("HTTP_SERVER_PORT", port.to_string())
        .env(
            "ABGEN_CATALYST_URL",
            env_or(
                "ABGEN_CATALYST_URL",
                format!("http://127.0.0.1:{preview_port}/content"),
            ),
        )
        .env(
            "ABGEN_WORLDS_CONTENT_URL",
            env_or(
                "ABGEN_WORLDS_CONTENT_URL",
                format!("{DEFAULT_CATALYST}/content"),
            ),
        )
        .env(
            "ABGEN_OUT_ROOT",
            env_or(
                "ABGEN_OUT_ROOT",
                cache_root.join("out").display().to_string(),
            ),
        )
        .env(
            "ABGEN_CACHE_DIR",
            env_or(
                "ABGEN_CACHE_DIR",
                cache_root.join("cache").display().to_string(),
            ),
        )
        .env(
            "RUST_LOG",
            env_or("RUST_LOG", "abgen=info,tower_http=warn".to_string()),
        )
        .kill_on_drop(true)
        .spawn();

    match spawned {
        Ok(mut child) => {
            let (tx, exited) = tokio::sync::watch::channel(false);
            tokio::spawn(async move {
                let _ = child.wait().await;
                let _ = tx.send(true);
            });
            Some(Sidecar { url, bin, exited })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            crate::ux::note_stderr(format!(
                "asset bundles off \u{2014} {bin} not found ({INSTALL_HINT})"
            ));
            None
        }
        Err(e) => {
            crate::ux::note_stderr(format!(
                "asset bundles off \u{2014} {bin} failed to start: {} ({INSTALL_HINT})",
                e.kind()
            ));
            None
        }
    }
}

impl Sidecar {
    pub async fn wait_ready(&mut self) -> bool {
        let ready_url = format!("{}/readyz", self.url);
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
        {
            Ok(c) => c,
            Err(_) => return false,
        };
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if *self.exited.borrow() {
                crate::ux::note_stderr(format!(
                    "asset bundles off \u{2014} {} exited before becoming ready ({INSTALL_HINT})",
                    self.bin
                ));
                return false;
            }
            if let Ok(res) = client.get(&ready_url).send().await {
                if res.status().is_success() {
                    return true;
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(READY_POLL_INTERVAL) => {}
                _ = self.exited.changed() => {}
            }
        }
        crate::ux::note_stderr(format!(
            "asset bundles off \u{2014} {} did not come up on {} within {}s ({INSTALL_HINT})",
            self.bin,
            self.url,
            READY_TIMEOUT.as_secs()
        ));
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_bin_precedence_is_env_then_embedded_then_npm_then_path() {
        let embedded = PathBuf::from("/tmp/embedded/abgen");
        let npm = PathBuf::from("/scene/node_modules/@dcl/abgen-linux-x64/abgen");
        assert_eq!(
            pick_bin(
                Some("/custom/abgen".into()),
                Some(embedded.clone()),
                Some(npm.clone())
            ),
            "/custom/abgen"
        );
        assert_eq!(
            pick_bin(None, Some(embedded.clone()), Some(npm.clone())),
            embedded.display().to_string()
        );
        assert_eq!(
            pick_bin(Some(String::new()), None, Some(npm.clone())),
            npm.display().to_string()
        );
        assert_eq!(pick_bin(None, None, None), "abgen");
    }

    #[test]
    fn npm_bin_finds_the_platform_package_binary() {
        let root = std::env::temp_dir().join(format!(
            "dcl-one-sdk-npm-bin-test-{}-{:x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        assert_eq!(npm_bin(&root), None);
        let platform = if cfg!(target_os = "windows") {
            "win32"
        } else if cfg!(target_os = "macos") {
            "darwin"
        } else {
            "linux"
        };
        let arch = if cfg!(target_arch = "x86_64") {
            "x64"
        } else {
            "arm64"
        };
        let bin_name = if cfg!(target_os = "windows") {
            "abgen.exe"
        } else {
            "abgen"
        };
        let pkg = root
            .join("node_modules")
            .join("@dcl")
            .join(format!("abgen-{platform}-{arch}"));
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join(bin_name), b"").unwrap();
        assert_eq!(npm_bin(&root), Some(pkg.join(bin_name)));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
