//! Disk LRU for upstream-fetched content under `<scene>/.dcl-cache/contents`.
//!
//! Everything that lands here is content-addressed (profile snapshots,
//! wearable GLBs and textures fetched through the `/content/contents`
//! catalyst fallback), so entries never go stale — the cache only bounds how
//! many are kept, evicting least-recently-used on insert. Recency is file
//! mtime, bumped on every hit; a `<hash>.ct` sidecar preserves the upstream
//! content type. The dot-dir is invisible to the watcher and to deploys.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const MAX_ENTRIES_ENV: &str = "DCL_ONE_SDK_CONTENT_CACHE_MAX";
const DEFAULT_MAX_ENTRIES: usize = 5000;

pub fn max_entries() -> usize {
    match std::env::var(MAX_ENTRIES_ENV) {
        Ok(v) if !v.trim().is_empty() => v.trim().parse().unwrap_or(DEFAULT_MAX_ENTRIES),
        _ => DEFAULT_MAX_ENTRIES,
    }
}

/// Catalyst hashes (Qm…/bafy…) are plain alphanumeric; anything else is not
/// cacheable and, more importantly, not a safe file name.
fn valid_hash(hash: &str) -> bool {
    !hash.is_empty() && hash.len() <= 128 && hash.bytes().all(|b| b.is_ascii_alphanumeric())
}

pub async fn get(dir: &Path, hash: &str) -> Option<(Vec<u8>, Option<String>)> {
    if !valid_hash(hash) || max_entries() == 0 {
        return None;
    }
    let dir = dir.to_path_buf();
    let hash = hash.to_string();
    tokio::task::spawn_blocking(move || {
        let path = dir.join(&hash);
        let bytes = std::fs::read(&path).ok()?;
        if let Ok(f) = std::fs::File::options().write(true).open(&path) {
            let _ = f.set_modified(SystemTime::now());
        }
        let ct = std::fs::read_to_string(dir.join(format!("{hash}.ct")))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Some((bytes, ct))
    })
    .await
    .ok()
    .flatten()
}

pub async fn put(dir: &Path, hash: &str, bytes: &[u8], content_type: Option<&str>) {
    let cap = max_entries();
    if !valid_hash(hash) || cap == 0 {
        return;
    }
    let dir = dir.to_path_buf();
    let hash = hash.to_string();
    let bytes = bytes.to_vec();
    let ct = content_type.map(str::to_string);
    let _ = tokio::task::spawn_blocking(move || {
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let tmp = dir.join(format!("{hash}.tmp{}", std::process::id()));
        if std::fs::write(&tmp, &bytes).is_err() || std::fs::rename(&tmp, dir.join(&hash)).is_err()
        {
            let _ = std::fs::remove_file(&tmp);
            return;
        }
        if let Some(ct) = ct.filter(|c| !c.is_empty()) {
            let _ = std::fs::write(dir.join(format!("{hash}.ct")), ct);
        }
        evict(&dir, cap);
    })
    .await;
}

fn evict(dir: &Path, cap: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<(SystemTime, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            if !valid_hash(&e.file_name().to_string_lossy()) {
                return None;
            }
            let md = e.metadata().ok()?;
            if !md.is_file() {
                return None;
            }
            Some((md.modified().ok()?, e.path()))
        })
        .collect();
    if entries.len() <= cap {
        return;
    }
    entries.sort_by_key(|(t, _)| *t);
    let excess = entries.len() - cap;
    for (_, path) in entries.into_iter().take(excess) {
        let _ = std::fs::remove_file(&path);
        let mut ct = path.into_os_string();
        ct.push(".ct");
        let _ = std::fs::remove_file(PathBuf::from(ct));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "dcl-one-sdk-content-cache-{tag}-{}-{:x}",
                std::process::id(),
                rand::random::<u64>()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Tmp(dir)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn hash_validation_refuses_traversal_and_local_hashes() {
        assert!(valid_hash(
            "bafkreicdlz2ab65lchjrciobzfsjsdjucydb2rudflujk5wxggp3h6443u"
        ));
        assert!(valid_hash("QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG"));
        assert!(!valid_hash("b64-abc"));
        assert!(!valid_hash("../etc/passwd"));
        assert!(!valid_hash("a.ct"));
        assert!(!valid_hash(""));
    }

    #[tokio::test]
    async fn put_get_roundtrip_preserves_bytes_and_content_type() {
        let tmp = Tmp::new("roundtrip");
        put(
            &tmp.0,
            "QmRoundTrip",
            b"glb-bytes",
            Some("model/gltf-binary"),
        )
        .await;
        assert_eq!(
            get(&tmp.0, "QmRoundTrip").await,
            Some((b"glb-bytes".to_vec(), Some("model/gltf-binary".into())))
        );
        assert_eq!(get(&tmp.0, "QmMissing").await, None);
        put(&tmp.0, "b64-notacid", b"x", None).await;
        assert!(!tmp.0.join("b64-notacid").exists());
    }

    #[test]
    fn evict_drops_least_recently_used_with_sidecars() {
        let tmp = Tmp::new("evict");
        let base = SystemTime::now() - Duration::from_secs(1000);
        for (i, name) in ["QmOld", "QmMid", "QmNew"].iter().enumerate() {
            let p = tmp.0.join(name);
            std::fs::write(&p, name).unwrap();
            std::fs::write(tmp.0.join(format!("{name}.ct")), "t/t").unwrap();
            std::fs::File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(base + Duration::from_secs(i as u64 * 10))
                .unwrap();
        }
        evict(&tmp.0, 2);
        assert!(!tmp.0.join("QmOld").exists());
        assert!(!tmp.0.join("QmOld.ct").exists());
        assert!(tmp.0.join("QmMid").exists());
        assert!(tmp.0.join("QmNew").exists());
        assert!(tmp.0.join("QmNew.ct").exists());
        evict(&tmp.0, 2);
        assert!(tmp.0.join("QmMid").exists());
    }
}
