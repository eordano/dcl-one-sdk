//! The abgen asset-bundle server, carried inside this binary.
//!
//! Every build embeds one (build.rs downloads the release pinned in
//! abgen-release.lock, or takes ABGEN_EMBED_BIN), so `start` always has a
//! sidecar to run and never asks a user to install anything. The files are
//! deflate-compressed in the binary and inflated into a temp directory keyed by
//! [`TAG`], a content hash — so a given build extracts once per machine, and a
//! new abgen lands in a new directory instead of racing the old one.

use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/abgen_embed_data.rs"));

pub fn present() -> bool {
    !FILES.is_empty()
}

/// Serializes extraction within the process. Two threads racing the same TAG
/// directory would otherwise each inflate 36 MB, and — before this — collide on
/// a staging name, since a pid is not unique between threads.
static EXTRACT: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn ensure_extracted() -> Option<PathBuf> {
    if FILES.is_empty() {
        return None;
    }
    let root = std::env::temp_dir().join("dcl-abgen").join("bin").join(TAG);
    let _guard = EXTRACT.lock().unwrap_or_else(|e| e.into_inner());
    match extract_into(&root) {
        Ok(()) => Some(root.join(BIN_NAME)),
        Err(e) => {
            crate::ux::note_stderr(format!(
                "embedded abgen could not be unpacked into {}: {e}",
                root.display()
            ));
            None
        }
    }
}

fn extract_into(root: &Path) -> std::io::Result<()> {
    for (rel, packed, raw_len) in FILES {
        let path = root.join(rel);
        if std::fs::metadata(&path).is_ok_and(|m| m.len() as usize == *raw_len) {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = inflate(packed, *raw_len)?;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let tmp = path.with_file_name(format!(
            ".{}.tmp-{}-{}",
            name.unwrap_or_default(),
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::write(&tmp, &bytes)?;
        if *rel == BIN_NAME || rel.starts_with("bin/") || rel.starts_with("lib/") {
            set_executable(&tmp)?;
        }
        std::fs::rename(&tmp, &path)?;
    }
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

    #[test]
    fn the_binary_carries_an_abgen() {
        assert!(
            present(),
            "every build embeds abgen; see abgen-release.lock"
        );
        assert!(!BIN_NAME.is_empty());
        assert!(!TAG.is_empty());
        assert!(FILES.iter().any(|(rel, _, _)| *rel == BIN_NAME));
    }

    #[test]
    fn extraction_yields_a_runnable_binary_and_is_idempotent() {
        let first = ensure_extracted().expect("embedded abgen extracts");
        assert!(first.is_file());
        let total: usize = FILES.iter().map(|(_, _, raw_len)| *raw_len).sum();
        assert!(total > 1_000_000, "embedded abgen bundle is {total} bytes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&first).unwrap();
            assert_eq!(meta.permissions().mode() & 0o111, 0o111);
        }
        assert_eq!(ensure_extracted().as_deref(), Some(first.as_path()));
    }
}
