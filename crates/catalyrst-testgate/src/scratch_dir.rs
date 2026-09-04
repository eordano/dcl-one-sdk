use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl std::ops::Deref for ScratchDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The `nonce` makes two `scratch_dir("same-tag")` calls from concurrent test
/// threads collision-free even though they share a pid, which the
/// hand-rolled `temp_dir().join(format!("...{}", process::id()))` call sites
/// this replaces cannot guarantee on their own.
pub fn scratch_dir(tag: &str) -> ScratchDir {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("catalyrst-{tag}-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&path).expect("create scratch fixture directory");
    ScratchDir { path }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_directory_exists_while_held_and_is_gone_after_drop() {
        let dir = scratch_dir("selftest-lifecycle");
        let path = dir.path().to_path_buf();
        assert!(path.is_dir());
        drop(dir);
        assert!(!path.exists());
    }

    #[test]
    fn repeat_calls_with_the_same_tag_never_collide() {
        let a = scratch_dir("selftest-collide");
        let b = scratch_dir("selftest-collide");
        assert_ne!(a.path(), b.path());
        assert!(a.path().is_dir());
        assert!(b.path().is_dir());
    }

    #[test]
    fn derefs_to_path_for_drop_in_use_at_call_sites() {
        let dir = scratch_dir("selftest-deref");
        fn wants_a_path(_p: &Path) {}
        wants_a_path(&dir);
    }
}
