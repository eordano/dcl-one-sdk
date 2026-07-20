use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/abgen_embed_data.rs"));

pub fn present() -> bool {
    !FILES.is_empty()
}

pub fn ensure_extracted() -> Option<PathBuf> {
    if FILES.is_empty() {
        return None;
    }
    let root = std::env::temp_dir().join("dcl-abgen").join("bin").join(TAG);
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
    for (rel, bytes) in FILES {
        let path = root.join(rel);
        if std::fs::read(&path).is_ok_and(|d| d == *bytes) {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        let tmp = path.with_file_name(format!(
            ".{}.tmp-{}",
            name.unwrap_or_default(),
            std::process::id()
        ));
        std::fs::write(&tmp, bytes)?;
        if *rel == BIN_NAME {
            set_executable(&tmp)?;
        }
        std::fs::rename(&tmp, &path)?;
    }
    Ok(())
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
    fn without_a_compile_time_embed_extraction_is_a_noop() {
        if present() {
            return;
        }
        assert!(ensure_extracted().is_none());
    }
}
