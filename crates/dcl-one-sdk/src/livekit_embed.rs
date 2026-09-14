//! The livekit-server carried inside this binary: build.rs embeds the release
//! pinned in livekit-release.lock (or LIVEKIT_EMBED_BIN; nothing on a target
//! LiveKit publishes no release for), one static executable, deflated. It is
//! inflated once per machine into a temp directory keyed by [`TAG`], a
//! content hash, so a new build never runs an old server.

use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/livekit_embed_data.rs"));

pub fn present() -> bool {
    !PACKED.is_empty()
}

/// Two threads extracting the same TAG would each inflate 47 MB and collide
/// on a staging name.
static EXTRACT: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn ensure_extracted() -> Option<PathBuf> {
    if PACKED.is_empty() {
        return None;
    }
    let path = std::env::temp_dir()
        .join("dcl-livekit")
        .join(TAG)
        .join(BIN_NAME);
    let _guard = EXTRACT.lock().unwrap_or_else(|e| e.into_inner());
    match extract_to(&path) {
        Ok(()) => Some(path),
        Err(e) => {
            crate::ux::note_stderr(format!(
                "embedded livekit-server could not be unpacked into {}: {e}",
                path.display()
            ));
            None
        }
    }
}

fn extract_to(path: &Path) -> std::io::Result<()> {
    if std::fs::metadata(path).is_ok_and(|m| m.len() as usize == RAW_LEN) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = inflate(PACKED, RAW_LEN)?;
    let tmp = path.with_file_name(format!(".{BIN_NAME}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, &bytes)?;
    set_executable(&tmp)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn inflate(packed: &[u8], raw_len: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::with_capacity(raw_len);
    flate2::read::DeflateDecoder::new(packed).read_to_end(&mut out)?;
    Ok(out)
}

#[cfg(unix)]
fn set_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn the_binary_carries_a_livekit_server() {
        if std::env::var("LIVEKIT_EMBED_BIN").is_ok_and(|v| v.eq_ignore_ascii_case("none")) {
            return;
        }
        assert!(
            present(),
            "livekit-release.lock names a release for this target, so PACKED must not be empty"
        );
        assert_ne!(VERSION, "none");
    }

    #[test]
    fn extraction_yields_a_runnable_server_and_is_idempotent() {
        if !present() {
            return;
        }
        let first = ensure_extracted().expect("extract");
        let modified = std::fs::metadata(&first).unwrap().modified().unwrap();
        let second = ensure_extracted().expect("extract again");
        assert_eq!(first, second);
        assert_eq!(
            std::fs::metadata(&second).unwrap().modified().unwrap(),
            modified,
            "a second call must not rewrite the file"
        );
        let out = std::process::Command::new(&first)
            .arg("--version")
            .output()
            .expect("run the extracted livekit-server");
        let text = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(text.contains("livekit-server version"), "{text}");
    }
}
